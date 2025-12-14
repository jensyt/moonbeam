use futures_lite::{AsyncRead, AsyncWrite};
use std::{
	future::Future,
	io::{self, Write},
	ops::{Range, RangeFrom},
	pin::Pin,
	task::{Context, Poll},
};

pub struct BodyWriteFuture<'a, 'b, S> {
	reader: Box<dyn AsyncRead + Unpin + 'static>,
	socket: &'a mut S,
	buf: &'b mut [u8],
	read_pos: usize,
	write_pos: usize,
	is_chunked: bool,
	read_done: bool,
}

impl<'a, 'b, S> BodyWriteFuture<'a, 'b, S>
where
	S: AsyncWrite + Unpin + 'static,
{
	pub fn new(
		buf: &'b mut [u8],
		preread: usize,
		body: Box<dyn AsyncRead + Unpin + 'static>,
		len: Option<usize>,
		socket: &'a mut S,
	) -> Self {
		Self {
			reader: body,
			socket,
			buf,
			read_pos: preread,
			write_pos: 0,
			is_chunked: len.is_none(),
			read_done: false,
		}
	}
}

impl<S> Future for BodyWriteFuture<'_, '_, S>
where
	S: AsyncWrite + Unpin + 'static,
{
	type Output = io::Result<()>;

	fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
		if self.is_chunked {
			self.poll_chunked(cx)
		} else {
			self.poll_not_chunked(cx)
		}
	}
}

impl<S> BodyWriteFuture<'_, '_, S>
where
	S: AsyncWrite + Unpin + 'static,
{
	fn poll_not_chunked(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
		let mself = self.get_mut();
		loop {
			let mut read_pending = false;

			match mself.poll_read_range_from(mself.read_pos.., cx)? {
				Poll::Ready(0) => mself.read_done = true,
				Poll::Ready(n) => mself.read_pos += n,
				Poll::Pending => read_pending = true,
			};

			let write_pending = mself.poll_write(cx)?.is_pending();

			if mself.read_done && mself.write_pos == mself.read_pos {
				return Poll::Ready(Ok(()));
			}

			if read_pending && write_pending {
				return Poll::Pending;
			}
		}
	}

	fn poll_chunked(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
		let mself = self.get_mut();
		loop {
			let mut read_pending = false;

			match mself.poll_read_range(mself.read_pos + 7..mself.buf.len() - 2, cx)? {
				Poll::Ready(0) => {
					mself.read_done = true;
					write!(&mut mself.buf[mself.read_pos..], "0\r\n\r\n")?;
					mself.read_pos += 5;
				}
				Poll::Ready(n) => {
					write!(&mut mself.buf[mself.read_pos..], "{n:0>5x}\r\n")?;
					mself.buf[mself.read_pos + 7 + n] = b'\r';
					mself.buf[mself.read_pos + 8 + n] = b'\n';
					mself.read_pos += n + 9
				}
				Poll::Pending => read_pending = true,
			};

			let write_pending = mself.poll_write(cx)?.is_pending();

			if mself.read_done && mself.write_pos == mself.read_pos {
				return Poll::Ready(Ok(()));
			}

			if read_pending && write_pending {
				return Poll::Pending;
			}
		}
	}

	fn poll_read_range(
		&mut self,
		range: Range<usize>,
		cx: &mut Context<'_>,
	) -> Poll<io::Result<usize>> {
		self.poll_read_impl(range.start, range.end, cx)
	}

	fn poll_read_range_from(
		&mut self,
		range: RangeFrom<usize>,
		cx: &mut Context<'_>,
	) -> Poll<io::Result<usize>> {
		self.poll_read_impl(range.start, self.buf.len(), cx)
	}

	fn poll_read_impl(
		&mut self,
		start: usize,
		end: usize,
		cx: &mut Context<'_>,
	) -> Poll<io::Result<usize>> {
		if !self.read_done && start < end {
			Pin::new(&mut self.reader).poll_read(cx, &mut self.buf[start..end])
		} else {
			Poll::Pending
		}
	}

	fn poll_write(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<usize>> {
		if self.write_pos < self.read_pos {
			let p =
				Pin::new(&mut self.socket).poll_write(cx, &self.buf[self.write_pos..self.read_pos]);
			match &p {
				Poll::Ready(Ok(0)) => {
					return Poll::Ready(Err(io::Error::new(
						io::ErrorKind::WriteZero,
						"failed to write to socket",
					)));
				}
				Poll::Ready(Ok(n)) => {
					self.write_pos += n;
					if self.write_pos == self.read_pos {
						self.read_pos = 0;
						self.write_pos = 0;
					} else {
						self.buf.copy_within(self.write_pos..self.read_pos, 0);
						self.read_pos -= self.write_pos;
						self.write_pos = 0;
					}
				}
				_ => (),
			}
			p
		} else {
			Poll::Pending
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use futures_lite::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
	use piper::{Reader, Writer};
	use std::pin::Pin;
	use std::task::{Context, Poll};

	// Helper to create a mock socket using piper
	struct MockSocket {
		reader: Reader,
		writer: Writer,
	}

	impl AsyncRead for MockSocket {
		fn poll_read(
			mut self: Pin<&mut Self>,
			cx: &mut Context<'_>,
			buf: &mut [u8],
		) -> Poll<std::io::Result<usize>> {
			Pin::new(&mut self.reader).poll_read(cx, buf)
		}
	}

	impl AsyncWrite for MockSocket {
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

	// Helper to create a stream body from a string
	fn stream_body(content: impl Into<Vec<u8>>) -> Box<dyn AsyncRead + Unpin + 'static> {
		let (reader, mut writer) = piper::pipe(1024);
		let content = content.into();
		let _len = content.len();

		// Spawn a task to write content to the pipe
		std::thread::spawn(move || {
			futures_lite::future::block_on(async move {
				writer.write_all(&content).await.unwrap();
				writer.close().await.unwrap();
			});
		});

		Box::new(reader)
	}

	#[test]
	fn test_body_write_future_known_length() {
		let (reader, _client_tx) = piper::pipe(1024);
		let (mut client_rx, writer) = piper::pipe(1024);
		let mut socket = MockSocket { reader, writer };

		let body_content = "Hello, World!";
		let body = stream_body(body_content);
		let mut buf = vec![0u8; 1024];

		// Fill buffer with some existing data to simulate preread
		let prefix = b"HTTP/1.1 200 OK\r\n\r\n";
		buf[..prefix.len()].copy_from_slice(prefix);
		let preread = prefix.len();

		let future = BodyWriteFuture::new(
			&mut buf,
			preread,
			body,
			Some(body_content.len()),
			&mut socket,
		);

		futures_lite::future::block_on(future).unwrap();
		futures_lite::future::block_on(socket.writer.close()).unwrap();

		futures_lite::future::block_on(async {
			let mut result = Vec::new();
			client_rx.read_to_end(&mut result).await.unwrap();
			let result_str = String::from_utf8(result).unwrap();

			// Expected output: Preread header + body content
			let expected = format!("{}{}", std::str::from_utf8(prefix).unwrap(), body_content);
			assert_eq!(result_str, expected);
		});
	}

	#[test]
	fn test_body_write_future_chunked() {
		let (reader, _client_tx) = piper::pipe(1024);
		let (mut client_rx, writer) = piper::pipe(1024);
		let mut socket = MockSocket { reader, writer };

		let body_content = "Chunked Data";
		let body = stream_body(body_content);
		let mut buf = vec![0u8; 1024];

		let prefix = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
		buf[..prefix.len()].copy_from_slice(prefix);
		let preread = prefix.len();

		// None for length triggers chunked encoding
		let future = BodyWriteFuture::new(&mut buf, preread, body, None, &mut socket);

		futures_lite::future::block_on(future).unwrap();
		futures_lite::future::block_on(socket.writer.close()).unwrap();

		futures_lite::future::block_on(async {
			let mut result = Vec::new();
			client_rx.read_to_end(&mut result).await.unwrap();
			let result_str = String::from_utf8(result).unwrap();

			let expected_start = std::str::from_utf8(prefix).unwrap();
			assert!(result_str.starts_with(expected_start));

			// Check for chunk structure: <hex len>\r\n<data>\r\n0\r\n\r\n
			let body_part = &result_str[expected_start.len()..];
			println!("Got body part: {:?}", body_part);
			assert!(body_part.ends_with("0\r\n\r\n"));
			assert!(body_part.contains(&format!(
				"{:x}\r\n{}\r\n",
				body_content.len(),
				body_content
			)));
		});
	}
}
