use crate::http::{Body, Request, Response, canonical_reason};
// use crate::server::bufpool::get_local_bufpool;
use crate::tracing;
use async_io::Timer;
use async_net::{AsyncToSocketAddrs, TcpListener};
use futures_lite::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, FutureExt};
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
use writer::BodyWriteFuture;

const BUFSIZE: usize = 16 * 1024;

// mod bufpool;
#[cfg(feature = "compress")]
mod compress;
mod parsing;
pub mod task;
#[cfg(feature = "signals")]
mod task_tracker;
mod writer;

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

#[cfg(feature = "signals")]
async fn accept_loop<T: Server>(listener: TcpListener, server: &'static T) {
	use async_signal::{Signal, Signals};
	use futures_lite::StreamExt;
	use task_tracker::get_local_tracker;

	let mut signals =
		Signals::new([Signal::Int, Signal::Term]).expect("Failed to create signal handler");

	loop {
		let signal_err = async {
			let signal = signals.next().await;
			Err(Error::new(
				ErrorKind::Interrupted,
				format!("Signal: {signal:?}"),
			))
		};

		match listener.accept().or(signal_err).await {
			Ok((socket, addr)) => {
				let _ = socket.set_nodelay(true);
				new_local_task(handle_socket(socket, addr, server));
			}
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

	let wait_for_tasks = get_local_tracker().wait_until_empty(Duration::from_secs(60));

	let force_shutdown = async {
		#[allow(unused_variables)]
		if let Some(signal) = signals.next().await {
			tracing::warn!("Received signal {:?}, forcing shutdown", signal);
		}
	};

	wait_for_tasks.or(force_shutdown).await;
}

#[cfg(not(feature = "signals"))]
async fn accept_loop<T: Server>(listener: TcpListener, server: &'static T) {
	loop {
		match listener.accept().await {
			Ok((socket, addr)) => {
				let _ = socket.set_nodelay(true);
				new_local_task(handle_socket(socket, addr, server));
			}
			#[allow(unused_variables)]
			Err(err) => {
				tracing::error!(?err, "Failed to accept connection, shutting down");
				break;
			}
		}
	}
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

			#[cfg(feature = "compress")]
			compress::apply_compression(&req, &mut resp);

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

	let nobody = match response.status {
		100..200 | 204 | 205 | 304 => true,
		_ => false,
	};

	let mut server = false;
	let mut date = false;
	let mut content_type = nobody;
	let mut content_length = nobody;

	for (name, value) in response.headers.iter() {
		if name.eq_ignore_ascii_case("server") {
			server = true;
		} else if name.eq_ignore_ascii_case("date") {
			date = true;
		} else if name.eq_ignore_ascii_case("content-type") {
			if nobody {
				continue;
			}
			content_type = true;
		} else if name.eq_ignore_ascii_case("content-length") {
			if nobody {
				continue;
			}
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

	if !content_type {
		match response.body.as_ref() {
			Some(_) => {
				writer.write(b"Content-Type: application/octet-stream\r\n")?;
			}
			None => (),
		}
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

	// Helper to create a stream body from a string
	fn stream_body(content: impl Into<Vec<u8>>) -> Box<dyn AsyncRead + Unpin + 'static> {
		let (reader, mut writer) = piper::pipe(1024);
		let content = content.into();

		std::thread::spawn(move || {
			futures_lite::future::block_on(async move {
				writer.write_all(&content).await.unwrap();
				writer.close().await.unwrap();
			});
		});

		Box::new(reader)
	}

	struct StreamServer;
	impl Server for StreamServer {
		async fn route(&'static self, req: Request<'_, '_>) -> Response {
			if req.path == "/stream" {
				let content = "Streamed Content";
				let body = Body::Stream {
					data: stream_body(content),
					len: Some(content.len() as u64),
				};
				Response::ok().with_body(body, None)
			} else if req.path == "/chunked" {
				let content = "Chunked Content";
				let body = Body::Stream {
					data: stream_body(content),
					len: None,
				};
				Response::ok().with_body(body, None)
			} else {
				Response::not_found()
			}
		}
	}

	#[test]
	fn test_handle_socket_stream_body_known_length() {
		let (reader, mut client_tx) = piper::pipe(1024);
		let (mut client_rx, writer) = piper::pipe(1024);
		let socket = MockStream { reader, writer };

		let server = Box::leak(Box::new(StreamServer));

		let handle_future = handle_socket(socket, "127.0.0.1:80".parse().unwrap(), server);

		let test_future = async move {
			client_tx
				.write_all(b"GET /stream HTTP/1.1\r\n\r\n")
				.await
				.unwrap();

			let mut buf = vec![0u8; 1024];
			let mut total_read = 0;
			loop {
				let n = client_rx.read(&mut buf[total_read..]).await.unwrap();
				if n == 0 {
					break;
				}
				total_read += n;
				if total_read >= buf.len() {
					break;
				}
				// Simple heuristic to stop reading if we got body
				if std::str::from_utf8(&buf[..total_read])
					.unwrap()
					.contains("Streamed Content")
				{
					break;
				}
			}
			let response = std::str::from_utf8(&buf[..total_read]).unwrap();

			assert!(response.contains("HTTP/1.1 200 OK"));
			assert!(response.contains("Content-Length: 16"));
			assert!(response.contains("Streamed Content"));
		};

		futures_lite::future::block_on(async {
			futures_lite::future::zip(handle_future, test_future).await;
		});
	}

	#[test]
	fn test_handle_socket_stream_body_chunked() {
		let (reader, mut client_tx) = piper::pipe(1024);
		let (mut client_rx, writer) = piper::pipe(1024);
		let socket = MockStream { reader, writer };

		let server = Box::leak(Box::new(StreamServer));

		let handle_future = handle_socket(socket, "127.0.0.1:80".parse().unwrap(), server);

		let test_future = async move {
			client_tx
				.write_all(b"GET /chunked HTTP/1.1\r\n\r\n")
				.await
				.unwrap();

			let mut buf = vec![0u8; 1024];
			let mut total_read = 0;
			loop {
				let n = client_rx.read(&mut buf[total_read..]).await.unwrap();
				if n == 0 {
					break;
				}
				total_read += n;
				if total_read >= buf.len() {
					break;
				}
				if std::str::from_utf8(&buf[..total_read])
					.unwrap()
					.ends_with("0\r\n\r\n")
				{
					break;
				}
			}
			let response = std::str::from_utf8(&buf[..total_read]).unwrap();

			assert!(response.contains("HTTP/1.1 200 OK"));
			assert!(response.contains("Transfer-Encoding: chunked"));
			assert!(response.contains("Chunked Content"));
			assert!(response.ends_with("0\r\n\r\n"));
		};

		futures_lite::future::block_on(async {
			futures_lite::future::zip(handle_future, test_future).await;
		});
	}

	#[test]
	fn test_header_stripping_304() {
		// Response with content headers but 304 status
		let response = Response::not_modified(Some("text/html"))
			.with_header("Content-Length", "100")
			.with_header("ETag", "\"123\"");

		let mut buffer = vec![0u8; 1024];
		let (head, _) = write_response(&response, &mut buffer).unwrap();
		let head_str = std::str::from_utf8(head).unwrap();

		assert!(head_str.contains("HTTP/1.1 304 Not Modified"));
		assert!(head_str.contains("ETag: \"123\""));
		assert!(!head_str.contains("Content-Type"));
		assert!(!head_str.contains("Content-Length"));
	}

	#[test]
	fn test_header_stripping_204() {
		// Response with content headers but 204 status
		let response = Response::empty()
			.with_header("Content-Type", "application/json")
			.with_header("Content-Length", "50");

		let mut buffer = vec![0u8; 1024];
		let (head, _) = write_response(&response, &mut buffer).unwrap();
		let head_str = std::str::from_utf8(head).unwrap();

		assert!(head_str.contains("HTTP/1.1 204 No Content"));
		assert!(!head_str.contains("Content-Type"));
		assert!(!head_str.contains("Content-Length"));
	}
}
