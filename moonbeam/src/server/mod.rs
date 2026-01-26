use crate::http::{Body, Request, Response, canonical_reason};
use crate::tracing::{self, Instrument};
use async_io::Timer;
use futures_lite::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, FutureExt};
use httparse::Header;
use httpdate::fmt_http_date;
use parsing::{get_important_headers, parse_http_request, scan_for_header_end};
#[cfg(feature = "catchpanic")]
use std::panic::AssertUnwindSafe;
use std::{
	borrow::Cow,
	io::{Error, ErrorKind, Read, Write},
	mem::MaybeUninit,
	net::SocketAddr,
	sync::OnceLock,
	time::{Duration, Instant, SystemTime},
};

const BUFSIZE: usize = 16 * 1024;

#[cfg(feature = "compress")]
mod compress;
#[cfg(feature = "mt")]
pub mod mt;
mod parsing;
pub mod st;
pub mod task;
#[cfg(feature = "signals")]
mod task_tracker;

/// Returns the maximum allowed size for an HTTP request body in bytes.
///
/// This value is read from the `MOONBEAM_MAX_BODY_SIZE` environment variable,
/// which is expected to be in Kilobytes (KB).
/// If the variable is not set or cannot be parsed as a `usize`, it defaults to 1024 KB (1MB).
/// The value is cached after the first read for performance.
fn max_body_size() -> usize {
	static SIZE: OnceLock<usize> = OnceLock::new();
	*SIZE.get_or_init(|| {
		std::env::var("MOONBEAM_MAX_BODY_SIZE")
			.ok()
			.and_then(|s| s.parse::<usize>().ok())
			.map(|kb| kb * 1024)
			.unwrap_or(1024 * 1024)
	})
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
	/// This function drops the static reference to self. It should only be called when the server
	/// is shutting down.
	unsafe fn destroy(&'static self) {
		unsafe {
			drop(Box::from_raw(&raw const *self as *mut Self));
		}
	}
}

macro_rules! socket_write {
	($e:expr) => {
		if let Err(_error) = $e.await {
			tracing::error!(?_error, "Failed to write response");
			return Err(());
		}
	};
}

/// Handles a single connection socket.
///
/// This function reads HTTP requests from the socket, routes them using the provided `router`,
/// and writes back the response. It handles:
/// - HTTP parsing
/// - Response compression (if enabled)
/// - Response writing
/// - Keep-alive connections
///
/// # Arguments
///
/// * `socket` - The connection socket (must implement `AsyncRead` and `AsyncWrite`).
/// * `addr` - The remote address of the connection (used for logging only).
/// * `router` - The server implementation to route requests to.
async fn handle_socket<R: Server, S>(mut socket: S, _addr: SocketAddr, router: &'static R)
where
	S: AsyncRead + AsyncWrite + Unpin + 'static,
{
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
				return;
			}
		};

		while let Some(n) = scan_for_header_end(&reqbuf[start..total]) {
			let offset = start + n;
			tracing::trace!(offset, total, "HTTP header read");

			let (head, body) = reqbuf.split_at_mut(offset);

			let mut headers = [MaybeUninit::<Header>::uninit(); 32];
			let req = match parse_http_request(head, &mut headers) {
				Err(_error) => {
					tracing::error!(?_error, "Failed to parse HTTP header");
					write_error_response(
						&mut socket,
						Response::bad_request().with_header("Connection", "close"),
						respbuf,
					)
					.await;
					return;
				}
				Ok(req) => req,
			};

			let (contentlength, close) = get_important_headers(&req);

			let result = process_request(
				req,
				&mut socket,
				router,
				respbuf,
				body,
				total - offset,
				contentlength,
				Instant::now(),
			)
			.instrument(tracing::info_span!(
				"request",
				method = req.method,
				path = req.path,
				remote = %_addr,
			))
			.await;

			if result.is_err() {
				tracing::trace!(reason = "Error", "Closing connection");
				return;
			}

			if close {
				tracing::trace!(
					reason = "Got Connection: close header",
					"Closing connection"
				);
				return;
			}

			// Reset the request buffer
			let reqlen = offset + contentlength;
			if reqlen < total {
				reqbuf.copy_within(reqlen..total, 0);
				total -= reqlen;
			} else {
				total = 0;
			}
			start = 0;
		}
	}

	write_error_response(
		&mut socket,
		Response::headers_too_large().with_header("Connection", "close"),
		respbuf,
	)
	.await;
	tracing::error!(
		error = "request headers too large",
		"Failed to read HTTP request"
	);
	tracing::trace!(reason = "Error", "Closing connection");
}

#[allow(clippy::too_many_arguments)]
async fn process_request<'a, 'b, R: Server, S>(
	mut req: Request<'a, 'b>,
	socket: &mut S,
	router: &'static R,
	respbuf: &mut [u8],
	body: &'b mut [u8],
	valid_body_len: usize,
	contentlength: usize,
	_start_time: Instant,
) -> Result<(), ()>
where
	S: AsyncRead + AsyncWrite + Unpin,
{
	tracing::trace!("Processing request");
	tracing::trace!(content_length = contentlength);

	let body = {
		if contentlength > max_body_size() {
			tracing::error!(
				content_length = contentlength,
				max_size = max_body_size(),
				"Failed to read HTTP body: too big"
			);
			write_error_response(
				socket,
				Response::content_too_large().with_header("Connection", "close"),
				respbuf,
			)
			.await;
			return Err(());
		}

		if contentlength > body.len() {
			let mut new_body = vec![0; contentlength];
			new_body[..valid_body_len].copy_from_slice(&body[..valid_body_len]);
			if let Err(_error) = socket.read_exact(&mut new_body[valid_body_len..]).await {
				tracing::error!(?_error, "Failed to read HTTP body");
				return Err(());
			}
			Cow::Owned(new_body)
		} else {
			if contentlength > valid_body_len
				&& let Err(_error) = socket
					.read_exact(&mut body[valid_body_len..contentlength])
					.await
			{
				tracing::error!(?_error, "Failed to read HTTP body");
				return Err(());
			}

			Cow::Borrowed(&body[..contentlength])
		}
	};
	req.body = &body;

	let head_method = req.method.eq_ignore_ascii_case("head");
	#[cfg(not(feature = "catchpanic"))]
	let mut resp = router.route(req).await;
	#[cfg(feature = "catchpanic")]
	let mut resp = match AssertUnwindSafe(router.route(req)).catch_unwind().await {
		Ok(resp) => resp,
		Err(_error) => {
			tracing::error!(?_error, "Panic in response handler");
			write_error_response(socket, Response::internal_server_error(), respbuf).await;
			// We can process additional requests after this, so return Ok
			return Ok(());
		}
	};

	#[cfg(feature = "compress")]
	compress::apply_compression(&req, &mut resp);

	tracing::info!(
		response.status = resp.status,
		response.content_type = resp
			.headers
			.iter()
			.find(|&(n, _)| n.eq_ignore_ascii_case("content-type"))
			.map(|(_, v)| v),
		response.body_len = resp.body.as_ref().and_then(|b| b.len()),
		latency_ms = _start_time.elapsed().as_millis() as u64,
		"Request processed"
	);

	let (head, mut rest) = match write_response(&resp, respbuf) {
		Ok(buf) => buf,
		Err(_error) => {
			tracing::error!(?_error, "Failed to write response");
			write_error_response(socket, Response::internal_server_error(), respbuf).await;
			// We can try to process additional requests after this, so return Ok
			return Ok(());
		}
	};

	if head_method {
		let _body = resp.body.take();
		tracing::trace!(removed_body = _body.is_some(), "Processing HEAD request");
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
			socket_write!(write_stream_body(socket, data, len, head));
			tracing::trace!(len, "Streamed body");
		}
	}

	Ok(())
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
				tracing::trace!("Remote closed connection");
			}
			Err(None)
		}
		Ok(n) => {
			tracing::trace!(n, total, "Successful socket read");
			Ok((total, total + n))
		}
		Err(error) => {
			if error.kind() == std::io::ErrorKind::TimedOut {
				tracing::trace!("Socket read timed out");
				Err(Some(
					Response::request_timeout().with_header("Connection", "close"),
				))
			} else {
				tracing::error!(?error, "Error reading socket");
				Err(None)
			}
		}
	}
}

fn write_response<'b>(
	response: &Response,
	buffer: &'b mut [u8],
) -> Result<(&'b [u8], &'b mut [u8]), Error> {
	let mut writer = &mut buffer[..];

	write!(
		writer,
		"HTTP/1.1 {} {}\r\n",
		response.status,
		canonical_reason(response.status)
	)?;

	let nobody = matches!(response.status, 100..200 | 204 | 205 | 304);

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
		writer.write_all(
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

	if !content_type && response.body.is_some() {
		writer.write_all(b"Content-Type: application/octet-stream\r\n")?;
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

	writer.write_all(b"\r\n")?;

	let writerlen = writer.len();
	let (header, remaining) = buffer.split_at_mut(buffer.len() - writerlen);
	Ok((header, remaining))
}

async fn write_stream_body<S>(
	socket: &mut S,
	data: Box<dyn Read + Send + 'static>,
	len: Option<u64>,
	head: &[u8],
) -> std::io::Result<()>
where
	S: AsyncWrite + Unpin,
{
	struct Buffer {
		data: Vec<u8>,
		len: usize,
	}

	let headlen = head.len();
	let mut respbufcopy = vec![0; BUFSIZE];
	respbufcopy[0..headlen].copy_from_slice(head);

	let (send_full, recv_full) = flume::bounded(2);
	let (send_empty, recv_empty) = flume::bounded(2);
	send_empty
		.send(Buffer {
			data: respbufcopy,
			len: headlen,
		})
		.unwrap();
	send_empty
		.send(Buffer {
			data: vec![0; BUFSIZE],
			len: 0,
		})
		.unwrap();

	let _reader = if len.is_none() {
		// Chunked transfer encoding
		blocking::unblock(move || -> std::io::Result<()> {
			let mut data = data;
			while let Ok(mut buf) = recv_empty.recv() {
				let start = buf.len;

				if BUFSIZE - start < 16 {
					if send_full.send(buf).is_err() {
						break;
					}
					continue;
				}

				let data_start = start + 7;
				let n = data.read(&mut buf.data[data_start..BUFSIZE - 2])?;

				if n == 0 {
					let term = b"0\r\n\r\n";
					buf.data[start..start + 5].copy_from_slice(term);
					buf.len += 5;
					let _ = send_full.send(buf);
					break;
				}

				let mut slice = &mut buf.data[start..data_start];
				write!(slice, "{:0>5x}\r\n", n).unwrap();

				buf.data[data_start + n] = b'\r';
				buf.data[data_start + n + 1] = b'\n';

				buf.len += 9 + n;

				if send_full.send(buf).is_err() {
					break;
				}
			}
			Ok(())
		})
	} else {
		// Known length
		blocking::unblock(move || -> std::io::Result<()> {
			let mut data = data;
			while let Ok(mut buf) = recv_empty.recv() {
				let n = if buf.len > 0 {
					data.read(&mut buf.data[buf.len..])?
				} else {
					data.read(&mut buf.data)?
				};

				if n == 0 {
					break;
				}

				buf.len += n;

				if send_full.send(buf).is_err() {
					break;
				}
			}
			Ok(())
		})
	};

	// Write filled buffers to the socket
	while let Ok(mut buf) = recv_full.recv_async().await {
		socket.write_all(&buf.data[0..buf.len]).await?;
		buf.len = 0;
		let _ = send_empty.send_async(buf).await;
	}

	// _reader will drop to ensure the background task is cancelled if writing the socket fails for
	// some reason.

	Ok(())
}

async fn write_error_response<W>(socket: &mut W, response: Response, buffer: &mut [u8])
where
	W: AsyncWrite + Unpin,
{
	let (head, _) = match write_response(&response, buffer) {
		Ok(buf) => buf,
		Err(_error) => {
			tracing::error!(?_error, "Failed to write response");
			return;
		}
	};

	if let Err(_error) = socket.write_all(head).await {
		tracing::error!(?_error, "Failed to write response");
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
		let response = Response::new_with_code(201)
			.with_header("X-Custom", "test")
			.with_body("test body", Body::DEFAULT_CONTENT_TYPE);

		let mut buffer = vec![0u8; 256];
		let result = write_response(&response, &mut buffer).unwrap();

		let response_str = std::str::from_utf8(result.0).unwrap();

		// Should contain status line
		assert!(response_str.contains("HTTP/1.1 201"));

		// Should contain custom header
		assert!(response_str.contains("X-Custom: test"));

		// Should contain default headers
		assert!(response_str.contains("Server: moonbeam/0.3"));
		assert!(response_str.contains("Content-Type: application/octet-stream"));
		assert!(response_str.contains("Content-Length: 9"));
		assert!(response_str.contains("Date:"));

		// Should end with \r\n\r\n
		assert!(response_str.ends_with("\r\n\r\n"));
	}

	#[test]
	fn test_write_response_custom_headers_override_defaults() {
		let response = Response::ok()
			.with_header("Server", "custom-server")
			.with_header("Content-Type", "text/plain");

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
		let response = Response::ok()
			.with_body("hello", Body::DEFAULT_CONTENT_TYPE)
			.with_header("Server", "custom-server")
			.with_header("Date", "Wed, 21 Oct 2015 07:28:00 GMT")
			.with_header("Content-Type", "text/plain")
			.with_header("Content-Length", "5");

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
			Response::ok().with_body(format!("Hello {}", req.path), Body::DEFAULT_CONTENT_TYPE)
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
	#[cfg(feature = "catchpanic")]
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

	struct StreamServer;
	impl Server for StreamServer {
		async fn route(&'static self, req: Request<'_, '_>) -> Response {
			if req.path == "/stream" {
				let content = "Streamed Content";
				let body = Body::Stream {
					len: Some(content.len() as u64),
					data: Box::new(std::io::Cursor::new(content)),
				};
				Response::ok().with_body(body, Body::DEFAULT_CONTENT_TYPE)
			} else if req.path == "/chunked" {
				let content = "Chunked Content";
				let body = Body::Stream {
					data: Box::new(std::io::Cursor::new(content)),
					len: None,
				};
				Response::ok().with_body(body, Body::DEFAULT_CONTENT_TYPE)
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
				.write_all(b"GET /chunked HTTP/1.1\r\nConnection: close\r\n\r\n")
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
			println!("{}", response);

			assert!(response.contains("HTTP/1.1 200 OK"));
			assert!(response.contains("Transfer-Encoding: chunked"));
			assert!(response.ends_with("f\r\nChunked Content\r\n0\r\n\r\n"));
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

	struct EchoServer;
	impl Server for EchoServer {
		async fn route(&'static self, req: Request<'_, '_>) -> Response {
			// Return body length as string
			let len = req.body.len();
			Response::ok().with_body(format!("{}", len), Body::DEFAULT_CONTENT_TYPE)
		}
	}

	#[test]
	fn test_handle_socket_large_body() {
		let (reader, mut client_tx) = piper::pipe(65536);
		let (mut client_rx, writer) = piper::pipe(1024);
		let socket = MockStream { reader, writer };

		let server = Box::leak(Box::new(EchoServer));

		let handle_future = handle_socket(socket, "127.0.0.1:80".parse().unwrap(), server);

		let body_size = 20 * 1024; // 20KB
		let body_content = vec![b'a'; body_size];

		let test_future = async move {
			let request_head = format!(
				"POST /echo HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
				body_size
			);
			client_tx.write_all(request_head.as_bytes()).await.unwrap();
			client_tx.write_all(&body_content).await.unwrap();

			let mut buf = vec![0u8; 1024];
			let n = client_rx.read(&mut buf).await.unwrap();
			let response = std::str::from_utf8(&buf[..n]).unwrap();

			assert!(response.contains("HTTP/1.1 200 OK"));
			// EchoServer returns body length
			assert!(response.ends_with(&format!("\r\n\r\n{}", body_size)));
		};

		futures_lite::future::block_on(async {
			futures_lite::future::zip(handle_future, test_future).await;
		});
	}

	#[test]
	fn test_handle_socket_too_large_body() {
		let (reader, mut client_tx) = piper::pipe(65536);
		let (mut client_rx, writer) = piper::pipe(1024);
		let socket = MockStream { reader, writer };

		let server = Box::leak(Box::new(EchoServer));

		let handle_future = handle_socket(socket, "127.0.0.1:80".parse().unwrap(), server);

		let body_size = 1024 * 1024 + 10; // 1MB + 10 bytes

		let test_future = async move {
			let request_head = format!(
				"POST /echo HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
				body_size
			);
			client_tx.write_all(request_head.as_bytes()).await.unwrap();
			// We don't need to write the full body to trigger the check,
			// the server checks Content-Length header first.

			let mut buf = vec![0u8; 1024];
			let n = client_rx.read(&mut buf).await.unwrap();
			let response = std::str::from_utf8(&buf[..n]).unwrap();

			assert!(response.contains("HTTP/1.1 413 Content Too Large"));
		};

		futures_lite::future::block_on(async {
			futures_lite::future::zip(handle_future, test_future).await;
		});
	}
}
