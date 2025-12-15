use crate::http::{Body, Request, Response, canonical_reason};
// use crate::server::bufpool::get_local_bufpool;
use crate::tracing;
use async_io::Timer;
use async_net::{AsyncToSocketAddrs, TcpListener};
use async_signal::{Signal, Signals};
use futures_lite::{
	AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, FutureExt, StreamExt,
	io::{BufReader, Cursor},
};
use httparse::Header;
use httpdate::fmt_http_date;
use parsing::{get_important_headers, parse_http_request, scan_for_header_end};
#[cfg(feature = "catchpanic")]
use std::panic::AssertUnwindSafe;
use std::{
	io::{Error, ErrorKind, Write},
	time::SystemTime,
};
use std::{mem::MaybeUninit, net::SocketAddr, time::Duration};
use task::{get_local_executor, new_local_task};
use task_tracker::get_local_tracker;
use writer::BodyWriteFuture;

const BUFSIZE: usize = 16 * 1024;

// mod bufpool;
mod parsing;
pub mod task;
mod task_tracker;
mod writer;

fn is_compressible(content_type: &str) -> bool {
	let ct = content_type.trim().to_ascii_lowercase();
	ct.starts_with("text/")
		|| ct.starts_with("application/json")
		|| ct.starts_with("application/xml")
		|| ct.starts_with("application/javascript")
		|| ct.starts_with("application/xhtml+xml")
		|| ct.starts_with("image/svg+xml")
		|| ct.starts_with("application/rss+xml")
		|| ct.starts_with("application/atom+xml")
}

/// Represents an HTTP server that can handle requests.
///
/// # Example
/// ```
/// use moonbeam::{Server, Request, Response};
/// use std::future::Future;
///
/// struct MyServer;
///
/// impl Server for MyServer {
///     fn route(&'static self, _req: Request) -> impl Future<Output = Response> {
///         async { Response::ok() }
///     }
/// }
/// ```
pub trait Server
where
	Self: 'static + Sized,
{
	/// Handles an incoming HTTP request and returns a future that resolves to a response.
	fn route(&'static self, request: Request) -> impl Future<Output = Response>;

	/// Clean up resources.
	///
	/// # Safety
	/// This function drops the static reference to self. It should only be called when the server is shutting down.
	unsafe fn destroy(&'static self) {
		unsafe {
			drop(Box::from_raw(&raw const *self as *mut Self));
		}
	}
}

/// Starts the server on the specified address.
///
/// This function blocks the current thread and runs the server loop.
/// It takes ownership of the server instance and leaks it to create a static reference,
/// which is required for the `Server` trait.
///
/// # Example
/// ```no_run
/// use moonbeam::{Server, Request, Response, serve};
/// use std::future::Future;
///
/// struct MyServer;
///
/// impl Server for MyServer {
///     fn route(&'static self, _req: Request) -> impl Future<Output = Response> {
///         async { Response::ok() }
///     }
/// }
///
/// serve("127.0.0.1:8080", MyServer);
/// ```
pub fn serve<T: Server>(addr: impl AsyncToSocketAddrs, server: T) -> &'static T {
	let static_server = Box::leak(Box::new(server));
	async_io::block_on(get_local_executor().run(async {
		let listener = TcpListener::bind(addr)
			.await
			.expect("Failed to bind to socket");
		accept_loop(listener, static_server).await;
	}));
	static_server
}

async fn accept_loop<T: Server>(listener: TcpListener, server: &'static T) {
	let mut signals =
		Signals::new([Signal::Int, Signal::Term]).expect("Failed to create signal handler");
	let mut get_signal = async move || {
		let signal = signals.next().await;
		Err(Error::new(
			ErrorKind::Interrupted,
			format!("Signal: {signal:?}"),
		))
	};

	loop {
		match listener.accept().or(get_signal()).await {
			Ok((socket, addr)) => new_local_task(handle_socket(socket, addr, server)),
			#[allow(unused_variables)]
			Err(err) => {
				if err.kind() == ErrorKind::Interrupted {
					tracing::debug!(?err, "Got signal to shut down");
				} else {
					tracing::error!(?err, "Failed to accept connection, shutting down");
				}
				break;
			}
		}
	}
	get_local_tracker()
		.wait_until_empty(Duration::from_secs(60))
		.await;
}

macro_rules! socket_write {
	($e:expr) => {
		if let Err(e) = $e.await {
			tracing::error!("Failed to write response: {:?}", e);
			break;
		}
	};
}

#[cfg_attr(not(feature = "tracing"), allow(unused_variables))]
#[cfg_attr(feature = "tracing", tracing::instrument(skip_all, fields(remote = %addr)))]
pub async fn handle_socket<R: Server, S>(mut socket: S, addr: SocketAddr, router: &'static R)
where
	S: AsyncRead + AsyncWrite + Unpin + 'static,
{
	// let mut buf = get_local_bufpool().get();
	// let (reqbuf, respbuf) = buf.get_mut().split_at_mut(bufpool::BUFSIZE / 2);
	let mut buf = vec![0; BUFSIZE];
	let (reqbuf, respbuf) = buf.split_at_mut(BUFSIZE / 2);
	let mut total = 0;

	while total < reqbuf.len() {
		let mut start = match read_from_socket(&mut socket, &mut reqbuf[total..], total).await {
			Ok((start, end)) => {
				total = end;
				start.saturating_sub(3)
			}
			Err(r) => {
				if let Some(r) = r {
					write_error_response(&mut socket, r, respbuf).await;
				}
				break;
			}
		};

		loop {
			let offset = match scan_for_header_end(&reqbuf[start..total]) {
				Some(n) => start + n,
				None => break,
			};
			tracing::trace!(offset, total, "HTTP header read");

			let (head, body) = reqbuf.split_at_mut(offset);

			let mut headers = [MaybeUninit::<Header>::uninit(); 32];
			let mut req = match parse_http_request(head, &mut headers) {
				Err(e) => {
					tracing::error!(error = ?e, "Failed to parse HTTP header");
					write_error_response(&mut socket, Response::bad_request(), respbuf).await;
					break;
				}
				Ok(req) => req,
			};

			let (contentlength, close) = get_important_headers(&req);
			tracing::trace!(contentlength);

			req.body = {
				if contentlength > body.len() {
					tracing::error!(error = "too big", "Failed to read HTTP body");
					write_error_response(&mut socket, Response::content_too_large(), respbuf).await;
					break;
				} else if contentlength > total - offset {
					if let Err(e) = socket
						.read_exact(&mut body[total - offset..contentlength])
						.await
					{
						tracing::error!(error = ?e, "Failed to read HTTP body");
						break;
					}
				}

				&body[..contentlength]
			};

			let path = req.path;
			let head_method = req.method.eq_ignore_ascii_case("head");
			#[cfg(not(feature = "catchpanic"))]
			let mut resp = router.route(req).await;
			#[cfg(feature = "catchpanic")]
			let mut resp = match AssertUnwindSafe(router.route(req)).catch_unwind().await {
				Ok(resp) => resp,
				Err(e) => {
					tracing::error!(request = %path, error = ?e, "Panic in response handler");
					write_error_response(&mut socket, Response::internal_server_error(), respbuf)
						.await;
					break;
				}
			};

			// Compression Logic
			#[cfg(feature = "compress")]
			{
				let already_compressed = resp
					.headers
					.iter()
					.any(|(n, _)| n.eq_ignore_ascii_case("content-encoding"));

				let compressible_type = resp
					.headers
					.iter()
					.find(|(n, _)| n.eq_ignore_ascii_case("content-type"))
					.map(|(_, v)| is_compressible(v))
					.unwrap_or(false);

				if !already_compressed && compressible_type {
					let accept_encoding = req
						.find_header("Accept-Encoding")
						.map(|v| String::from_utf8_lossy(v).to_string())
						.unwrap_or_default();

					let use_brotli = accept_encoding.contains("br");
					let use_gzip = accept_encoding.contains("gzip");

					if use_brotli || use_gzip {
						// Small body optimization threshold (4KB)
						const SMALL_BODY_THRESHOLD: usize = 4096;

						let mut new_body = None;
						let mut stream_body = false;

						if let Some(Body::Immediate(ref data)) = resp.body {
							if data.len() < SMALL_BODY_THRESHOLD {
								// Synchronous compression for small bodies
								let compressed = if use_brotli {
									let mut compressed_data = Vec::with_capacity(data.len());
									let res = {
										let mut encoder = brotli::CompressorWriter::new(
											&mut compressed_data,
											4096, // buffer size
											6,    // quality
											20,   // lgwin
										);
										encoder.write_all(data)
									};
									// writer is dropped here, finalizing the stream
									if res.is_ok() {
										Some(compressed_data)
									} else {
										None
									}
								} else {
									let mut encoder = flate2::write::GzEncoder::new(
										Vec::with_capacity(data.len()),
										flate2::Compression::default(),
									);
									encoder
										.write_all(data)
										.ok()
										.and_then(|_| encoder.finish().ok())
								};

								if let Some(c) = compressed {
									new_body = Some(Body::Immediate(c));
									// Remove potentially stale Content-Length so the writer recalculates it
									resp.headers
										.retain(|(n, _)| !n.eq_ignore_ascii_case("content-length"));
								}
							} else {
								stream_body = true;
							}
						} else if resp.body.is_some() {
							stream_body = true;
						}

						if stream_body {
							// Stream compression for large bodies or streams
							resp.headers
								.retain(|(n, _)| !n.eq_ignore_ascii_case("content-length"));

							let body_stream: Box<dyn AsyncRead + Unpin + 'static> =
								match resp.body.take() {
									Some(Body::Immediate(data)) => Box::new(Cursor::new(data)),
									Some(Body::Stream { data, .. }) => data,
									None => Box::new(Cursor::new(vec![])),
								};

							// Wrap in BufReader to satisfy AsyncBufRead
							let reader = BufReader::new(body_stream);

							let compressed_stream: Box<dyn AsyncRead + Unpin + 'static> =
								if use_brotli {
									Box::new(
										async_compression::futures::bufread::BrotliEncoder::new(
											reader,
										),
									)
								} else {
									Box::new(async_compression::futures::bufread::GzipEncoder::new(
										reader,
									))
								};

							new_body = Some(Body::Stream {
								data: compressed_stream,
								len: None,
							});
						}

						if let Some(b) = new_body {
							resp.body = Some(b);
							// Only set Content-Encoding if we actually compressed
							let encoding = if use_brotli { "br" } else { "gzip" };
							resp.set_header("Content-Encoding", encoding);
						}
					}
				}
			}

			tracing::info!(
				request = %path,
				response.status = resp.status,
				response.content_type = resp
					.headers
					.iter()
					.find(|&(n, _)| n.eq_ignore_ascii_case("content-type"))
					.map(|(_, v)| v),
				response.body = ?resp.body
			);

			let (head, mut rest) = match write_response(&resp, respbuf) {
				Ok(buf) => buf,
				Err(e) => {
					tracing::error!("Failed to write response: {:?}", e);
					break;
				}
			};

			if head_method {
				let body = resp.body.take();
				tracing::trace!(removed_body = body.is_some(), "Processing HEAD request");
			}

			match resp.body {
				None => {
					socket_write!(socket.write_all(head));
					tracing::trace!("Wrote headers only");
				}
				Some(Body::Immediate(body)) if body.len() < rest.len() => {
					let _ = rest.write_all(body.as_slice());
					let len = head.len() + body.len();
					socket_write!(socket.write_all(&respbuf[..len]));
					tracing::trace!("Wrote headers and body in one shot");
				}
				Some(Body::Immediate(body)) => {
					socket_write!(socket.write_all(head));
					socket_write!(socket.write_all(body.as_slice()));
					tracing::trace!(body_len = body.len(), "Wrote headers and body separately");
				}
				Some(Body::Stream { data, len }) => {
					let preread = head.len();
					socket_write!(BodyWriteFuture::new(
						respbuf,
						preread,
						data,
						len.map(|v| v as usize),
						&mut socket,
					));
					tracing::trace!(len, "Streamed body");
				}
			}

			if close {
				tracing::trace!("Got Connection: close header");
				break;
			}

			// Reset the request buffer
			let reqlen = offset + contentlength;
			reqbuf.copy_within(reqlen..total, 0);
			total -= reqlen;
			start = 0;
		}
	}

	if total == reqbuf.len() {
		write_error_response(&mut socket, Response::headers_too_large(), respbuf).await;
		tracing::error!(
			error = "request headers too large",
			"Failed to read HTTP request"
		);
	}

	tracing::debug!("Shutting down socket");
	let _ = socket.close().await;
}

async fn read_from_socket<R>(
	socket: &mut R,
	buf: &mut [u8],
	total: usize,
) -> Result<(usize, usize), Option<Response>>
where
	R: AsyncRead + Unpin,
{
	match socket
		.read(buf)
		.or(async {
			Timer::after(Duration::from_secs(30)).await;
			Err(Error::new(ErrorKind::TimedOut, "Timeout"))
		})
		.await
	{
		Ok(0) => {
			if total > 0 {
				tracing::warn!(unused_bytes = total, "Remote closed connection");
			} else {
				tracing::info!("Remote closed connection");
			}
			Err(None)
		}
		Ok(n) => {
			tracing::trace!(n, total, "Successful socket read");
			Ok((total, total + n))
		}
		Err(e) => {
			if e.kind() == std::io::ErrorKind::TimedOut {
				tracing::warn!("Socket read timed out");
				Err(Some(
					Response::request_timeout().with_header("Connection", "close"),
				))
			} else {
				tracing::error!(error = ?e, "Error reading socket");
				Err(None)
			}
		}
	}
}

fn write_response<'a, 'b>(
	response: &'a Response,
	buffer: &'b mut [u8],
) -> Result<(&'b [u8], &'b mut [u8]), Error> {
	let mut writer = &mut buffer[..];

	write!(
		writer,
		"HTTP/1.1 {} {}\r\n",
		response.status,
		canonical_reason(response.status)
	)?;

	let mut server = false;
	let mut date = false;
	let mut content_type = false;
	let mut content_length = false;

	for (name, value) in response.headers.iter() {
		if name.eq_ignore_ascii_case("server") {
			server = true;
		} else if name.eq_ignore_ascii_case("date") {
			date = true;
		} else if name.eq_ignore_ascii_case("content-type") {
			content_type = true;
		} else if name.eq_ignore_ascii_case("content-length") {
			content_length = true;
		}

		write!(writer, "{}: {}\r\n", name, value)?;
	}

	// Add headers
	if !server {
		writer.write(
			concat!(
				"Server: ",
				env!("CARGO_PKG_NAME"),
				"/",
				env!("CARGO_PKG_VERSION"),
				"\r\n"
			)
			.as_bytes(),
		)?;
	}
	if !date {
		write!(writer, "Date: {}\r\n", fmt_http_date(SystemTime::now()))?;
	}
	let nobody = match response.status {
		100..200 | 204 | 205 | 304 => true,
		_ => false,
	};
	if !nobody {
		let hasbody = match response.body.as_ref() {
			Some(body) => match body.len() {
				Some(len) if len > 0 => true,
				_ => false,
			},
			None => false,
		};
		if !content_type && hasbody {
			writer.write(b"Content-Type: application/octet-stream\r\n")?;
		}
		if !content_length {
			match response.body.as_ref() {
				Some(body) => match body.len() {
					Some(len) => write!(writer, "Content-Length: {}\r\n", len)?,
					None => write!(writer, "Transfer-Encoding: chunked\r\n")?,
				},
				None => write!(writer, "Content-Length: 0\r\n")?,
			}
		}
	}

	writer.write(b"\r\n")?;

	let writerlen = writer.len();
	let (header, remaining) = buffer.split_at_mut(buffer.len() - writerlen);
	Ok((header, remaining))
}

async fn write_error_response<W>(socket: &mut W, response: Response, buffer: &mut [u8])
where
	W: AsyncWrite + Unpin,
{
	let (head, _) = match write_response(&response, buffer) {
		Ok(buf) => buf,
		#[allow(unused_variables)]
		Err(e) => {
			tracing::error!("Failed to write response: {:?}", e);
			return;
		}
	};

	#[allow(unused_variables)]
	if let Err(e) = socket.write_all(head).await {
		tracing::error!("Failed to write response: {:?}", e);
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use futures_lite::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
	use piper::{Reader, Writer};
	use std::pin::Pin;
	use std::task::{Context, Poll};

	#[test]
	fn test_write_response() {
		let response = Response {
			status: 201,
			headers: vec![("X-Custom".to_string(), "test".to_string())],
			body: Some(b"test body".to_vec().into()),
		};

		let mut buffer = vec![0u8; 256];
		let result = write_response(&response, &mut buffer).unwrap();

		let response_str = std::str::from_utf8(result.0).unwrap();

		// Should contain status line
		assert!(response_str.contains("HTTP/1.1 201"));

		// Should contain custom header
		assert!(response_str.contains("X-Custom: test"));

		// Should contain default headers
		assert!(response_str.contains("Server: moonbeam/0.2"));
		assert!(response_str.contains("Content-Type: application/octet-stream"));
		assert!(response_str.contains("Content-Length: 9"));
		assert!(response_str.contains("Date:"));

		// Should end with \r\n\r\n
		assert!(response_str.ends_with("\r\n\r\n"));
	}

	#[test]
	fn test_write_response_custom_headers_override_defaults() {
		let response = Response {
			status: 200,
			headers: vec![
				("Server".to_string(), "custom-server".to_string()),
				("Content-Type".to_string(), "text/plain".to_string()),
			],
			body: None,
		};

		let mut buffer = vec![0u8; 256];
		let result = write_response(&response, &mut buffer).unwrap();

		let response_str = std::str::from_utf8(result.0).unwrap();

		// Should use custom headers instead of defaults
		assert!(response_str.contains("Server: custom-server"));
		assert!(response_str.contains("Content-Type: text/plain"));

		// Should not contain default server header
		assert!(!response_str.contains("Server: moonbeam/0.1"));
		assert!(!response_str.contains("Content-Type: application/octet-stream"));
	}

	#[test]
	fn test_write_response_all_default_headers_set() {
		let response = Response {
			status: 200,
			headers: vec![
				("Server".to_string(), "custom-server".to_string()),
				(
					"Date".to_string(),
					"Wed, 21 Oct 2015 07:28:00 GMT".to_string(),
				),
				("Content-Type".to_string(), "text/plain".to_string()),
				("Content-Length".to_string(), "5".to_string()),
			],
			body: Some(b"hello".to_vec().into()),
		};

		let mut buffer = vec![0u8; 512];
		let result = write_response(&response, &mut buffer).unwrap();
		let response_str = std::str::from_utf8(result.0).unwrap();

		// Should only contain custom headers, no duplicate defaults
		assert!(response_str.contains("Server: custom-server"));
		assert!(response_str.contains("Date: Wed, 21 Oct 2015 07:28:00 GMT"));
		assert!(response_str.contains("Content-Type: text/plain"));
		assert!(response_str.contains("Content-Length: 5"));

		// Count occurrences to ensure no duplicates
		assert_eq!(response_str.matches("Server:").count(), 1);
		assert_eq!(response_str.matches("Date:").count(), 1);
		assert_eq!(response_str.matches("Content-Type:").count(), 1);
		assert_eq!(response_str.matches("Content-Length:").count(), 1);
	}

	#[test]
	fn test_write_response_empty_body() {
		let response = Response::empty();

		let mut buffer = vec![0u8; 128];
		let result = write_response(&response, &mut buffer).unwrap();

		let response_str = std::str::from_utf8(result.0).unwrap();

		assert!(response_str.contains("HTTP/1.1 204"));
		assert!(!response_str.contains("Content-Length"));
	}

	struct MockStream {
		reader: Reader,
		writer: Writer,
	}

	impl AsyncRead for MockStream {
		fn poll_read(
			mut self: Pin<&mut Self>,
			cx: &mut Context<'_>,
			buf: &mut [u8],
		) -> Poll<std::io::Result<usize>> {
			Pin::new(&mut self.reader).poll_read(cx, buf)
		}
	}

	impl AsyncWrite for MockStream {
		fn poll_write(
			mut self: Pin<&mut Self>,
			cx: &mut Context<'_>,
			buf: &[u8],
		) -> Poll<std::io::Result<usize>> {
			Pin::new(&mut self.writer).poll_write(cx, buf)
		}

		fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
			Pin::new(&mut self.writer).poll_flush(cx)
		}

		fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
			Pin::new(&mut self.writer).poll_close(cx)
		}
	}

	struct MockServer;
	impl Server for MockServer {
		async fn route(&'static self, req: Request<'_, '_>) -> Response {
			if req.path == "/error" {
				panic!("forced panic");
			}
			Response::ok().with_body(format!("Hello {}", req.path), None)
		}
	}

	#[test]
	fn test_handle_socket_simple_request() {
		let (reader, mut client_tx) = piper::pipe(1024);
		let (mut client_rx, writer) = piper::pipe(1024);
		let socket = MockStream { reader, writer };

		let server = Box::leak(Box::new(MockServer));

		let handle_future = handle_socket(socket, "127.0.0.1:80".parse().unwrap(), server);

		let test_future = async move {
			client_tx
				.write_all(b"GET /world HTTP/1.1\r\n\r\n")
				.await
				.unwrap();

			let mut buf = vec![0u8; 1024];
			let n = client_rx.read(&mut buf).await.unwrap();
			let response = std::str::from_utf8(&buf[..n]).unwrap();

			assert!(response.contains("HTTP/1.1 200 OK"));
			assert!(response.contains("Hello /world"));
		};

		futures_lite::future::block_on(async {
			futures_lite::future::zip(handle_future, test_future).await;
		});
	}

	#[test]
	fn test_handle_socket_keep_alive() {
		let (reader, mut client_tx) = piper::pipe(1024);
		let (mut client_rx, writer) = piper::pipe(1024);
		let socket = MockStream { reader, writer };

		let server = Box::leak(Box::new(MockServer));

		let handle_future = handle_socket(socket, "127.0.0.1:80".parse().unwrap(), server);

		let test_future = async move {
			client_tx
				.write_all(b"GET /one HTTP/1.1\r\n\r\n")
				.await
				.unwrap();

			let mut buf = [0u8; 1024];
			let n = client_rx.read(&mut buf).await.unwrap();
			let response = std::str::from_utf8(&buf[..n]).unwrap();
			assert!(response.contains("Hello /one"));

			client_tx
				.write_all(b"GET /two HTTP/1.1\r\n\r\n")
				.await
				.unwrap();

			let n = client_rx.read(&mut buf).await.unwrap();
			let response = std::str::from_utf8(&buf[..n]).unwrap();
			assert!(response.contains("Hello /two"));
		};

		futures_lite::future::block_on(async {
			futures_lite::future::zip(handle_future, test_future).await;
		});
	}

	#[test]
	fn test_handle_socket_malformed() {
		let (reader, mut client_tx) = piper::pipe(1024);
		let (mut client_rx, writer) = piper::pipe(1024);
		let socket = MockStream { reader, writer };
		let server = Box::leak(Box::new(MockServer));

		let handle_future = handle_socket(socket, "127.0.0.1:80".parse().unwrap(), server);

		let test_future = async move {
			client_tx.write_all(b"GARBAGE\r\n\r\n").await.unwrap();

			let mut buf = [0u8; 1024];
			let n = client_rx.read(&mut buf).await.unwrap();
			let response = std::str::from_utf8(&buf[..n]).unwrap();

			assert!(response.contains("400 Bad Request"));
		};

		futures_lite::future::block_on(async {
			futures_lite::future::zip(handle_future, test_future).await;
		});
	}

	#[test]
	fn test_handle_socket_route_panic() {
		let (reader, mut client_tx) = piper::pipe(1024);
		let (mut client_rx, writer) = piper::pipe(1024);
		let socket = MockStream { reader, writer };
		let server = Box::leak(Box::new(MockServer));

		let handle_future = handle_socket(socket, "127.0.0.1:80".parse().unwrap(), server);

		let test_future = async move {
			client_tx
				.write_all(b"GET /error HTTP/1.1\r\n\r\n")
				.await
				.unwrap();

			let mut buf = [0u8; 1024];
			let n = client_rx.read(&mut buf).await.unwrap();
			let response = std::str::from_utf8(&buf[..n]).unwrap();

			assert!(response.contains("500 Internal Server Error"));
		};

		futures_lite::future::block_on(async {
			futures_lite::future::zip(handle_future, test_future).await;
		});
	}

	#[test]
	fn test_serve() {
		use async_net::TcpListener;
		use std::time::Duration;

		// Pick a random port by binding to 0 and getting the address
		let addr = futures_lite::future::block_on(async {
			let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
			listener.local_addr().unwrap()
		});

		let server = MockServer;

		let serve_addr = addr.clone();
		// Spawn server in a thread
		std::thread::spawn(move || {
			serve(serve_addr, server);
		});

		// Give it a moment to start
		std::thread::sleep(Duration::from_millis(100));

		// Connect
		futures_lite::future::block_on(async {
			let mut stream = async_net::TcpStream::connect(addr).await.unwrap();
			stream
				.write_all(b"GET /serve HTTP/1.1\r\n\r\n")
				.await
				.unwrap();

			let mut buf = vec![0u8; 1024];
			let n = stream.read(&mut buf).await.unwrap();
			let response = std::str::from_utf8(&buf[..n]).unwrap();

			assert!(response.contains("Hello /serve"));
		});
	}
}
#[cfg(test)]
mod tests_compression {
	use super::*;
	use futures_lite::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
	use piper::{Reader, Writer};
	use std::io::Read;
	use std::pin::Pin;
	use std::task::{Context, Poll};

	struct MockStream {
		reader: Reader,
		writer: Writer,
	}

	impl AsyncRead for MockStream {
		fn poll_read(
			mut self: Pin<&mut Self>,
			cx: &mut Context<'_>,
			buf: &mut [u8],
		) -> Poll<std::io::Result<usize>> {
			Pin::new(&mut self.reader).poll_read(cx, buf)
		}
	}

	impl AsyncWrite for MockStream {
		fn poll_write(
			mut self: Pin<&mut Self>,
			cx: &mut Context<'_>,
			buf: &[u8],
		) -> Poll<std::io::Result<usize>> {
			Pin::new(&mut self.writer).poll_write(cx, buf)
		}

		fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
			Pin::new(&mut self.writer).poll_flush(cx)
		}

		fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
			Pin::new(&mut self.writer).poll_close(cx)
		}
	}

	struct MockServer {
		body: Vec<u8>,
		content_type: String,
		use_stream: bool,
	}

	impl Server for MockServer {
		async fn route(&'static self, _req: Request<'_, '_>) -> Response {
			let body = if self.use_stream {
				Body::Stream {
					data: Box::new(Cursor::new(self.body.clone())),
					len: Some(self.body.len() as u64),
				}
			} else {
				Body::Immediate(self.body.clone())
			};

			Response::ok().with_body(body, Some(&self.content_type))
		}
	}

	async fn run_test(server: MockServer, accept_encoding: Option<&str>) -> (String, Vec<u8>) {
		let (reader, mut client_tx) = piper::pipe(65536);
		let (mut client_rx, writer) = piper::pipe(65536);
		let socket = MockStream { reader, writer };
		let server = Box::leak(Box::new(server));

		let handle_future = handle_socket(socket, "127.0.0.1:80".parse().unwrap(), server);

		let test_future = async move {
			let mut headers = "GET / HTTP/1.1\r\n".to_string();
			if let Some(enc) = accept_encoding {
				headers.push_str(&format!("Accept-Encoding: {}\r\n", enc));
			}
			headers.push_str("\r\n");

			client_tx.write_all(headers.as_bytes()).await.unwrap();
			client_tx.close().await.unwrap();

			let mut buf = Vec::new();
			client_rx.read_to_end(&mut buf).await.unwrap();
			buf
		};

		// Run until test_future completes. handle_socket might loop, but test_future closes connection.
		// However, handle_socket loops until socket read error (which close produces).
		// We can race them.

		// Actually, handle_socket returns when socket closes.
		let (_, buf) = futures_lite::future::zip(handle_future, test_future).await;

		// Parse response manually
		let mut headers = [httparse::EMPTY_HEADER; 32];
		let mut resp = httparse::Response::new(&mut headers);
		let status = resp.parse(&buf).unwrap();

		let body_start = status.unwrap();
		let head_str = String::from_utf8_lossy(&buf[..body_start]).to_string();
		let body_bytes = buf[body_start..].to_vec();

		(head_str, body_bytes)
	}

	fn decode_gzip(data: &[u8]) -> Vec<u8> {
		let mut d = flate2::read::GzDecoder::new(data);
		let mut s = Vec::new();
		d.read_to_end(&mut s).unwrap();
		s
	}

	fn decode_brotli(data: &[u8]) -> Vec<u8> {
		let mut d = brotli::Decompressor::new(data, 4096);
		let mut s = Vec::new();
		d.read_to_end(&mut s).unwrap();
		s
	}

	fn decode_chunked(data: &[u8]) -> Vec<u8> {
		// Simple chunked decoder for testing
		let mut res = Vec::new();
		let mut cur = std::io::Cursor::new(data);
		loop {
			let mut line = String::new();
			let mut char_buf = [0u8; 1];
			loop {
				if cur.read(&mut char_buf).unwrap() == 0 {
					return res;
				}
				let c = char_buf[0] as char;
				line.push(c);
				if line.ends_with("\r\n") {
					break;
				}
			}
			let len_str = line.trim();
			if len_str.is_empty() {
				continue;
			}
			let len = usize::from_str_radix(len_str, 16).unwrap();
			if len == 0 {
				break;
			}

			let mut chunk = vec![0u8; len];
			cur.read_exact(&mut chunk).unwrap();
			res.extend_from_slice(&chunk);

			// consume \r\n
			let mut dump = [0u8; 2];
			cur.read_exact(&mut dump).unwrap();
		}
		res
	}

	#[test]
	fn test_compress_small_gzip() {
		let body = b"hello world".to_vec();
		let server = MockServer {
			body: body.clone(),
			content_type: "text/plain".to_string(),
			use_stream: false,
		};

		let (head, resp_body) = futures_lite::future::block_on(run_test(server, Some("gzip")));

		assert!(head.contains("Content-Encoding: gzip"));
		// Should have Content-Length because it's small and sync compressed
		assert!(head.contains("Content-Length"));

		let decoded = decode_gzip(&resp_body);
		assert_eq!(decoded, body);
	}

	#[test]
	fn test_compress_small_brotli() {
		let body = b"hello world".to_vec();
		let server = MockServer {
			body: body.clone(),
			content_type: "text/plain".to_string(),
			use_stream: false,
		};

		let (head, resp_body) = futures_lite::future::block_on(run_test(server, Some("br")));

		assert!(head.contains("Content-Encoding: br"));
		assert!(head.contains("Content-Length"));

		let decoded = decode_brotli(&resp_body);
		assert_eq!(decoded, body);
	}

	#[test]
	fn test_compress_large_gzip_stream() {
		// Large body > 4KB
		let body = vec![b'a'; 10000];
		let server = MockServer {
			body: body.clone(),
			content_type: "text/plain".to_string(),
			use_stream: false, // Immediate but large
		};

		let (head, resp_body) = futures_lite::future::block_on(run_test(server, Some("gzip")));

		assert!(head.contains("Content-Encoding: gzip"));
		assert!(head.contains("Transfer-Encoding: chunked"));
		assert!(!head.contains("Content-Length"));

		let chunk_decoded = decode_chunked(&resp_body);
		let decoded = decode_gzip(&chunk_decoded);
		assert_eq!(decoded, body);
	}

	#[test]
	fn test_compress_stream_brotli() {
		let body = b"stream me".to_vec();
		let server = MockServer {
			body: body.clone(),
			content_type: "text/plain".to_string(),
			use_stream: true, // Force stream
		};

		let (head, resp_body) = futures_lite::future::block_on(run_test(server, Some("br")));

		assert!(head.contains("Content-Encoding: br"));
		assert!(head.contains("Transfer-Encoding: chunked"));

		let chunk_decoded = decode_chunked(&resp_body);
		let decoded = decode_brotli(&chunk_decoded);
		assert_eq!(decoded, body);
	}

	#[test]
	fn test_no_compression_unsupported_type() {
		let body = b"binary".to_vec();
		let server = MockServer {
			body: body.clone(),
			content_type: "application/octet-stream".to_string(),
			use_stream: false,
		};

		let (head, resp_body) = futures_lite::future::block_on(run_test(server, Some("gzip")));

		assert!(!head.contains("Content-Encoding"));
		assert_eq!(resp_body, body);
	}

	#[test]
	fn test_preference_br_over_gzip() {
		let body = b"hello".to_vec();
		let server = MockServer {
			body: body.clone(),
			content_type: "text/plain".to_string(),
			use_stream: false,
		};

		// Client accepts both
		let (head, _) = futures_lite::future::block_on(run_test(server, Some("gzip, br")));
		assert!(head.contains("Content-Encoding: br"));

		// Client accepts only gzip
		let (head_gz, _) = futures_lite::future::block_on(run_test(
			MockServer {
				body: body.clone(),
				content_type: "text/plain".to_string(),
				use_stream: false,
			},
			Some("gzip"),
		));
		assert!(head_gz.contains("Content-Encoding: gzip"));
	}
}
