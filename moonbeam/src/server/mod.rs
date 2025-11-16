use crate::http::{Body, Request, Response, canonical_reason};
// use crate::server::bufpool::get_local_bufpool;
use crate::tracing;
use async_io::Timer;
use async_net::{AsyncToSocketAddrs, TcpListener, TcpStream};
use async_signal::{Signal, Signals};
use futures_lite::{AsyncReadExt, AsyncWriteExt, FutureExt, StreamExt};
use httparse::Header;
use httpdate::fmt_http_date;
use parsing::{get_important_headers, parse_http_request, scan_for_header_end};
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
mod task;
mod task_tracker;

pub trait Server
where
	Self: 'static + Sized,
{
	fn route(&'static self, request: Request) -> impl Future<Output = Response>;

	fn serve(self, addr: impl AsyncToSocketAddrs) -> &'static Self {
		let static_self = Box::leak(Box::new(self));
		async_io::block_on(get_local_executor().run(async {
			// get_local_executor()
			// 	.spawn(async {
			// 		let mut t = Timer::interval(Duration::from_secs(5 * 60));
			// 		while let Some(_) = t.next().await {
			// 			get_local_bufpool().reset();
			// 		}
			// 	})
			// 	.detach();

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
					Ok((socket, addr)) => new_local_task(handle_socket(socket, addr, static_self)),
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
		static_self
	}

	unsafe fn destroy(&'static self) {
		unsafe {
			(&raw const *self as *mut Self).drop_in_place();
		}
	}
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
			_ => break,
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
					break;
				}
				Ok(req) => req,
			};

			let (contentlength, close) = get_important_headers(&req);
			tracing::trace!(contentlength);

			req.body = {
				if contentlength > body.len() {
					tracing::error!(error = "too big", "Failed to read HTTP body");
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
			let head = req.method.eq_ignore_ascii_case("head");
			let resp = router.route(req).await;

			tracing::info!(
				request = %path,
				response.status = resp.status,
				response.content_type = resp
					.headers
					.iter()
					.find(|&(n, _)| n.eq_ignore_ascii_case("content-type"))
					.map(|(_, v)| v),
				response.len = resp.body.as_ref().map(|b| b.len())
			);

			let respbuf = match write_response(&resp, respbuf) {
				Ok(buf) => buf,
				Err(e) => {
					tracing::error!("Failed to write response: {:?}", e);
					break;
				}
			};

			if let Err(e) = socket.write_all(respbuf).await {
				tracing::error!("Failed to write response: {:?}", e);
				break;
			}

			if !head && let Some(body) = resp.body
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

	tracing::debug!("Shutting down socket");
	let _ = socket.shutdown(std::net::Shutdown::Both);
}

async fn read_from_socket(
	mut socket: TcpStream,
	buf: &mut [u8],
	total: usize,
) -> Result<(usize, usize), ()> {
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
			Err(())
		}
		Ok(n) => {
			tracing::trace!(n, total, "Successful socket read");
			Ok((total, total + n))
		}
		Err(e) => {
			if e.kind() == std::io::ErrorKind::TimedOut {
				tracing::warn!("Socket read timed out");
			} else {
				tracing::error!(error = ?e, "Error reading socket");
			}
			Err(())
		}
	}
}

fn write_response<'a, 'b>(response: &'a Response, buffer: &'b mut [u8]) -> Result<&'b [u8], Error> {
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
		writer.write(b"Server: moonbeam/0.1\r\n")?;
	}
	if !date {
		write!(writer, "Date: {}\r\n", fmt_http_date(SystemTime::now()))?;
	}
	let nobody = match response.status {
		100..200 | 204 | 205 | 304 => true,
		_ => false,
	};
	if !nobody {
		if !content_type {
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
	Ok(&buffer[..buffer.len() - writerlen])
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
			write!($buf, "{bytesread:0>5}\r\n")?;
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

async fn write_response_body(body: Body, socket: &mut TcpStream) -> Result<(), Error> {
	const {
		assert!(
			BUFSIZE < 100000,
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

		let response_str = std::str::from_utf8(result).unwrap();

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

		let response_str = std::str::from_utf8(result).unwrap();

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
		let response_str = std::str::from_utf8(result).unwrap();

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

		let response_str = std::str::from_utf8(result).unwrap();

		assert!(response_str.contains("HTTP/1.1 204"));
		assert!(!response_str.contains("Content-Length"));
	}
}
