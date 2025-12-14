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
