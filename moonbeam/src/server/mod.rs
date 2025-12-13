use crate::http::{Body, Request, Response, canonical_reason};
// use crate::server::bufpool::get_local_bufpool;
use crate::tracing;
use async_io::Timer;
use async_net::{AsyncToSocketAddrs, TcpListener, TcpStream};
use async_signal::{Signal, Signals};
use futures_lite::{AsyncReadExt, AsyncWrite, AsyncWriteExt, FutureExt, StreamExt};
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

const BUFSIZE: usize = 16 * 1024;

// mod bufpool;
mod parsing;
pub mod task;
mod task_tracker;

pub trait Server
where
	Self: 'static + Sized,
{
	fn route(&'static self, request: Request) -> impl Future<Output = Response>;

	unsafe fn destroy(&'static self) {
		unsafe {
			drop(Box::from_raw(&raw const *self as *mut Self));
		}
	}
}

pub fn serve<T: Server>(addr: impl AsyncToSocketAddrs, server: T) -> &'static T {
	let static_server = Box::leak(Box::new(server));
	async_io::block_on(get_local_executor().run(async {
		let mut signals =
			Signals::new([Signal::Int, Signal::Term]).expect("Failed to create signal handler");
		let mut get_signal = async move || {
			let signal = signals.next().await;
			Err(Error::new(
				ErrorKind::Interrupted,
				format!("Signal: {signal:?}"),
			))
		};

		let listener = TcpListener::bind(addr)
			.await
			.expect("Failed to bind to socket");
		loop {
			match listener.accept().or(get_signal()).await {
				Ok((socket, addr)) => new_local_task(handle_socket(socket, addr, static_server)),
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
	}));
	static_server
}

#[cfg_attr(not(feature = "tracing"), allow(unused_variables))]
#[cfg_attr(feature = "tracing", tracing::instrument(skip_all, fields(remote = %addr)))]
pub async fn handle_socket<R: Server>(mut socket: TcpStream, addr: SocketAddr, router: &'static R) {
	// let mut buf = get_local_bufpool().get();
	// let (reqbuf, respbuf) = buf.get_mut().split_at_mut(bufpool::BUFSIZE / 2);
	let mut buf = vec![0; BUFSIZE];
	let (reqbuf, respbuf) = buf.split_at_mut(BUFSIZE / 2);
	let mut total = 0;

	while total < reqbuf.len() {
		let mut start = match read_from_socket(socket.clone(), &mut reqbuf[total..], total).await {
			Ok((start, end)) => {
				total = end;
				start.saturating_sub(3)
			}
			Err(r) => {
				if let Some(r) = r {
					write_error_response(socket.clone(), r, respbuf).await;
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
					write_error_response(socket.clone(), Response::bad_request(), respbuf).await;
					break;
				}
				Ok(req) => req,
			};

			let (contentlength, close) = get_important_headers(&req);
			tracing::trace!(contentlength);

			req.body = {
				if contentlength > body.len() {
					tracing::error!(error = "too big", "Failed to read HTTP body");
					write_error_response(socket.clone(), Response::content_too_large(), respbuf)
						.await;
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
					write_error_response(
						socket.clone(),
						Response::internal_server_error(),
						respbuf,
					)
					.await;
					break;
				}
			};

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

			let (len, wrote_body) = if !head_method
				&& let Some(body) = resp.body.as_mut()
				&& let Some(len) = body.len()
				&& len < rest.len() as u64
			{
				let len = len as usize;
				match body {
					Body::Immediate(body) => match rest.write_all(body.as_slice()) {
						Ok(_) => (head.len() + len, true),
						Err(_) => (head.len(), false),
					},
					Body::Sync { data, len: _ } => match data.read_exact(&mut rest[..len]) {
						Ok(_) => (head.len() + len, true),
						Err(_) => (head.len(), false),
					},
					Body::Async { data, len: _ } => match data.read_exact(&mut rest[..len]).await {
						Ok(_) => (head.len() + len, true),
						Err(_) => (head.len(), false),
					},
				}
			} else {
				(head.len(), false)
			};
			tracing::trace!(
				one_shot = resp.body.is_none() || wrote_body || head_method,
				"Wrote response to socket in one-shot"
			);

			if let Err(e) = socket.write_all(&respbuf[..len]).await {
				tracing::error!("Failed to write response: {:?}", e);
				break;
			}

			if !head_method
				&& !wrote_body
				&& let Some(body) = resp.body
				&& let Err(e) = write_response_body(body, &mut socket).await
			{
				tracing::error!("Failed to write response body: {:?}", e);
				break;
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
		write_error_response(socket.clone(), Response::headers_too_large(), respbuf).await;
	}

	tracing::debug!("Shutting down socket");
	let _ = socket.shutdown(std::net::Shutdown::Both);
}

async fn read_from_socket(
	mut socket: TcpStream,
	buf: &mut [u8],
	total: usize,
) -> Result<(usize, usize), Option<Response>> {
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

async fn write_error_response(mut socket: TcpStream, response: Response, buffer: &mut [u8]) {
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

macro_rules! stream_body {
	($socket:ident, $data:ident, $len:ident, async = $a:tt) => {{
		// let mut buf = get_local_bufpool().get();
		// let mut buf = buf.get_mut();
		let mut buf = vec![0; BUFSIZE];
		match $len {
			Some(_) => stream_body!(_notchunked $socket, $data, $len, buf, $a),
			None => stream_body!(_chunked $socket, $data, $len, buf, $a),
		}

		Ok(())
	}};
	(_notchunked $socket:ident, $data:ident, $len:ident, $buf:ident, $a:tt) => {
		loop {
			let bytesread = stream_body!(_read_notchunked $data, $buf, $a)?;
			if bytesread == 0 {
				break;
			}
			$socket.write_all(&$buf[..bytesread]).await?;
		}
	};
	(_chunked $socket:ident, $data:ident, $len:ident, $buf:ident, $a:tt) => {
		loop {
			// We need to write chunk information before/after the data itself
			let bytesread = stream_body!(_read_chunked $data, $buf, $a)?;
			if bytesread == 0 {
				$socket.write_all(b"0\r\n\r\n").await?;
				break;
			}
			{
				let mut prefix = &mut $buf[0..7];
				write!(prefix, "{bytesread:0>5x}\r\n")?;
			}
			$buf[bytesread + 7] = b'\r';
			$buf[bytesread + 8] = b'\n';
			let start = $buf.iter().position(|&v| v != b'0').unwrap_or(0);
			$socket.write_all(&$buf[start..bytesread + 9]).await?;
		}
	};
	(_read_notchunked $data:ident, $buf:ident, true) => {
		$data.read(&mut $buf).await
	};
	(_read_notchunked $data:ident, $buf:ident, false) => {
		$data.read(&mut $buf)
	};
	(_read_chunked $data:ident, $buf:ident, true) => {
		$data.read(&mut $buf[7..BUFSIZE-2]).await
	};
	(_read_chunked $data:ident, $buf:ident, false) => {
		$data.read(&mut $buf[7..BUFSIZE-2])
	};
}

async fn write_response_body<W>(body: Body, socket: &mut W) -> Result<(), Error>
where
	W: AsyncWrite + Unpin,
{
	const {
		assert!(
			BUFSIZE <= 0xFFFFF,
			"Buffer size too large, this function assumes 5 printed characters max"
		);
	}
	match body {
		Body::Immediate(body) => socket.write_all(body.as_slice()).await,
		Body::Sync { mut data, len } => stream_body!(socket, data, len, async = false),
		Body::Async { mut data, len } => stream_body!(socket, data, len, async = true),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

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
		assert!(response_str.contains("Server: moonbeam/0.1"));
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
	#[test]
	fn test_write_response_body_immediate() {
		let body = Body::Immediate(vec![1, 2, 3, 4]);
		let mut socket = Vec::new();

		let result = futures_lite::future::block_on(write_response_body(body, &mut socket));
		assert!(result.is_ok());
		assert_eq!(socket, vec![1, 2, 3, 4]);
	}

	#[test]
	fn test_write_response_body_sync() {
		let data = std::io::Cursor::new(vec![5, 6, 7, 8]);
		let body = Body::Sync {
			data: Box::new(data),
			len: Some(4),
		};
		let mut socket = Vec::new();

		let result = futures_lite::future::block_on(write_response_body(body, &mut socket));
		assert!(result.is_ok());
		assert_eq!(socket, vec![5, 6, 7, 8]);
	}

	#[test]
	fn test_write_response_body_sync_no_len_chunked() {
		let data = std::io::Cursor::new(vec![9, 10]);
		let body = Body::Sync {
			data: Box::new(data),
			len: None, // Triggers chunked encoding
		};
		let mut socket = Vec::new();

		let result = futures_lite::future::block_on(write_response_body(body, &mut socket));
		assert!(result.is_ok());

		// The buffer size is 16KB. The entire data fits in one read.
		// Chunked encoding format: size in hex \r\n data \r\n 0 \r\n \r\n
		// The macro uses loops and reads buffers.
		// Since we provided 2 bytes, it should read 2 bytes.
		// 2 in hex is 2.
		// Expected: "2\r\n\x09\x0A\r\n0\r\n\r\n"
		// The macro formats hex with 0>5x, but then skips leading '0's.

		let expected = b"2\r\n\x09\x0A\r\n0\r\n\r\n";
		assert_eq!(socket, expected);
	}

	#[test]
	fn test_write_response_body_async() {
		let data = futures_lite::io::Cursor::new(vec![11, 12, 13]);
		let body = Body::Async {
			data: Box::new(data),
			len: Some(3),
		};
		let mut socket = Vec::new();

		let result = futures_lite::future::block_on(write_response_body(body, &mut socket));
		assert!(result.is_ok());
		assert_eq!(socket, vec![11, 12, 13]);
	}

	#[test]
	fn test_write_response_body_async_no_len_chunked() {
		let data = futures_lite::io::Cursor::new(vec![14, 15, 16]);
		let body = Body::Async {
			data: Box::new(data),
			len: None,
		};
		let mut socket = Vec::new();

		let result = futures_lite::future::block_on(write_response_body(body, &mut socket));
		assert!(result.is_ok());

		// 3 bytes.
		// "00003\r\n\x0E\x0F\x10\r\n0\r\n\r\n" -> "3\r\n\x0E\x0F\x10\r\n0\r\n\r\n" (leading zeros skipped)
		let expected = b"3\r\n\x0E\x0F\x10\r\n0\r\n\r\n";
		assert_eq!(socket, expected);
}
}
