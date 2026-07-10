//! Utilities to turn an async function into [`AsyncRead`].
//!
//! Moonbeam supports async streamed bodies via [`Body::AsyncStream`](super::Body::AsyncStream). To
//! improve ergonomics, this module provides utilities that let an async function act like a
//! generator, yielding results to the underlying socket using [`AsyncStreamWriter`].
//!
//! # Example
//!
//! ```
//! use moonbeam::{server, AsyncStreamWriter, Body, Response, Request, Spawner};
//!
//! #[server(AsyncStreamServer)]
//! async fn handler(_request: Request, _spawner: Spawner) -> Response {
//!     Response::ok().with_body(
//!         Body::from_stream_fn(async |writer: AsyncStreamWriter| {
//!             writer.write(b"async stream").await;
//!         }),
//!         Body::TEXT,
//!     )
//! }
//! ```

use futures_lite::{AsyncRead, Future};
use std::cell::UnsafeCell;
use std::pin::Pin;
use std::rc::Rc;
use std::task::{Context, Poll};

const BUFFER_SIZE: usize = 1024;

/// Wrapper for an async function to act as a [`futures_lite::AsyncRead`] stream.
pub(super) struct AsyncStreamFn<F> {
	f: Option<F>,
	buffer: Rc<UnsafeCell<Buffer<BUFFER_SIZE>>>,
}

impl<F> AsyncStreamFn<F> {
	/// Creates a new `AsyncStreamFn` adapter wrapping the given async fn.
	pub fn new<R>(async_stream_fn: R) -> Self
	where
		R: FnOnce(AsyncStreamWriter) -> F,
	{
		let buffer = Rc::new(UnsafeCell::new(Buffer::new()));
		Self {
			f: Some(async_stream_fn(AsyncStreamWriter::new(buffer.clone()))),
			buffer,
		}
	}
}

impl<F> AsyncRead for AsyncStreamFn<F>
where
	F: Future<Output = ()>,
{
	fn poll_read(
		self: Pin<&mut Self>,
		cx: &mut Context<'_>,
		buf: &mut [u8],
	) -> Poll<std::io::Result<usize>> {
		let (buffer, mut f) = unsafe {
			let this = self.get_unchecked_mut();
			(this.buffer.get(), Pin::new_unchecked(&mut this.f))
		};
		let mut amt = None;
		loop {
			// SAFETY: `buffer` is guaranteed to be valid and non-null. `Rc` guarantees we cannot
			// have data race conditions because it makes all our types !Send.
			if unsafe { !(*buffer).is_empty } {
				let n = unsafe { (*buffer).copy_to(buf) };
				let m = amt.unwrap_or(0) + n;
				if n == 0 || m >= buf.len() {
					return Poll::Ready(Ok(m));
				}
				amt = Some(m);
			}

			// Try to move the underlying future forward
			if let Some(innerf) = f.as_mut().as_pin_mut() {
				match innerf.poll(cx) {
					Poll::Pending => {
						// Check if anything was added to the buffer
						if unsafe { (*buffer).is_empty } {
							match amt {
								None => return Poll::Pending,
								Some(n) => return Poll::Ready(Ok(n)),
							}
						}
					}
					Poll::Ready(_) => {
						// Make sure we don't poll the future again
						f.set(None);
					}
				}
			} else if unsafe { (*buffer).is_empty } {
				return Poll::Ready(Ok(amt.unwrap_or(0)));
			}
		}
	}
}

/// Async stream writer to use from an async stream fn.
///
/// This writer lets an async function 'yield' results to write to the underlying socket. You cannot
/// create this writer directly, it is provided by the framework when using helper functions like
/// [`Body::from_stream_fn`](super::Body::from_stream_fn) and
/// [`Response::new_from_sse_fn`](super::Response::new_from_sse_fn).
///
/// # Example
///
/// ```
/// use moonbeam::{server, AsyncStreamWriter, Body, Response, Request, Spawner};
///
/// #[server(AsyncStreamServer)]
/// async fn handler(_request: Request, _spawner: Spawner) -> Response {
///     Response::ok().with_body(
///         Body::from_stream_fn(async |writer: AsyncStreamWriter| {
///             writer.write(b"async stream").await;
///         }),
///         Body::TEXT,
///     )
/// }
/// ```
pub struct AsyncStreamWriter {
	buffer: Rc<UnsafeCell<Buffer<BUFFER_SIZE>>>,
}

impl AsyncStreamWriter {
	fn new(buffer: Rc<UnsafeCell<Buffer<BUFFER_SIZE>>>) -> Self {
		Self { buffer }
	}

	/// Write to the output socket.
	///
	/// This function does not write anything itself, it returns a `Future` to asynchronously
	/// write data to the output socket.
	pub fn write<'a>(&self, data: &'a impl AsRef<[u8]>) -> AsyncStreamWriteFuture<'a> {
		AsyncStreamWriteFuture {
			src: data.as_ref(),
			dst: self.buffer.clone(),
		}
	}

	/// Write a string to the output socket.
	///
	/// This function calls `write` after converting the input data to a String.
	pub async fn write_string(&self, data: impl ToString) {
		self.write(&data.to_string()).await
	}
}

/// A `Future` that writes data to the output socket.
#[must_use = "Futures do nothing if not polled"]
pub struct AsyncStreamWriteFuture<'a> {
	src: &'a [u8],
	dst: Rc<UnsafeCell<Buffer<BUFFER_SIZE>>>,
}

impl Future for AsyncStreamWriteFuture<'_> {
	type Output = ();
	fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
		let this = self.get_mut();
		// SAFETY: `dst` is guaranteed to be valid and non-null. `Rc` guarantees we cannot have data
		// race conditions because it makes all our types !Send.
		let n = unsafe { (*this.dst.get()).copy_from(this.src) };
		if n >= this.src.len() {
			Poll::Ready(())
		} else {
			this.src = &this.src[n..];
			// Note: typically a future that returns pending needs to trigger a wakeup, but we rely
			// on AsyncStreamFn to continuously poll us until everything is written.
			Poll::Pending
		}
	}
}

/// Circular buffer with fixed capacity
struct Buffer<const N: usize> {
	buffer: [u8; N],
	head: usize,
	tail: usize,
	is_empty: bool,
}

impl<const N: usize> Buffer<N> {
	fn new() -> Self {
		Self {
			buffer: [0u8; N],
			head: 0,
			tail: 0,
			is_empty: true,
		}
	}

	/// Copy data from the buffer into `target`.
	/// Returns the number of bytes written to `target`.
	fn copy_to(&mut self, target: &mut [u8]) -> usize {
		if self.is_empty || target.is_empty() {
			return 0;
		}

		// Calculate how many bytes are available to read
		let available = if self.head < self.tail {
			// Head precedes tail: readable region is in the middle, no wrap
			self.tail - self.head
		} else if self.head > self.tail {
			// Tail precedes head: readable region wraps around the end
			(N - self.head) + self.tail
		} else {
			// head == tail and not empty → buffer is completely full
			N
		};

		let to_copy = available.min(target.len());

		// Copy from head to the end of the buffer (or up to to_copy)
		let first_part = (N - self.head).min(to_copy);
		target[..first_part].copy_from_slice(&self.buffer[self.head..self.head + first_part]);

		// If we still need more, wrap around and copy from the start
		if first_part < to_copy {
			let second_part = to_copy - first_part;
			target[first_part..to_copy].copy_from_slice(&self.buffer[..second_part]);
		}

		self.head = (self.head + to_copy) % N;
		self.is_empty = self.head == self.tail;

		to_copy
	}

	/// Copy data from `src` into the buffer.
	/// Returns the number of bytes written (consumed from `src`).
	fn copy_from(&mut self, src: &[u8]) -> usize {
		if src.is_empty() {
			return 0;
		}

		// Calculate how many bytes can be written
		let available = if self.is_empty {
			// Buffer is empty: the full capacity is writable
			N
		} else if self.tail < self.head {
			// Tail precedes head: writable region is in the middle
			self.head - self.tail
		} else if self.tail > self.head {
			// Head precedes tail: writable region wraps around
			(N - self.tail) + self.head
		} else {
			// tail == head and not empty → buffer is completely full
			0
		};

		let to_copy = available.min(src.len());
		if to_copy == 0 {
			return 0;
		}

		// Write at the tail position
		let first_part = (N - self.tail).min(to_copy);
		self.buffer[self.tail..self.tail + first_part].copy_from_slice(&src[..first_part]);

		// Wrap around if needed
		if first_part < to_copy {
			let second_part = to_copy - first_part;
			self.buffer[..second_part].copy_from_slice(&src[first_part..to_copy]);
		}

		self.tail = (self.tail + to_copy) % N;
		self.is_empty = false;

		to_copy
	}

	#[cfg(test)]
	fn len(&self) -> usize {
		if self.is_empty {
			0
		} else if self.head < self.tail {
			self.tail - self.head
		} else {
			self.buffer.len() - self.head + self.tail
		}
	}
}

#[cfg(test)]
mod test {
	use super::*;
	use futures_lite::{AsyncReadExt, future::block_on};
	use std::pin::pin;

	#[test]
	fn test_asyncstream() {
		let mut stream = pin!(AsyncStreamFn::new(async |writer| {
			writer.write(b"Test ").await;
			writer.write(b"data").await;
		}));
		let mut buf = Vec::new();
		let result = block_on(stream.read_to_end(&mut buf));

		assert!(result.is_ok());
		assert_eq!(result.unwrap(), 9);
		assert_eq!(buf, b"Test data");
	}

	#[test]
	fn test_asyncstream_big_write() {
		let chunk_a = [0xAAu8; BUFFER_SIZE + 64];
		let chunk_b = [0xBBu8; BUFFER_SIZE + 128];
		let mut stream = pin!(AsyncStreamFn::new(async |writer| {
			writer.write(&chunk_a).await;
			writer.write(&chunk_b).await;
		}));
		let mut buf = Vec::new();
		let result = block_on(stream.read_to_end(&mut buf));

		assert!(result.is_ok());
		assert_eq!(result.unwrap(), chunk_a.len() + chunk_b.len());
		assert!(buf[..chunk_a.len()].iter().all(|&b| b == 0xAA));
		assert!(buf[chunk_a.len()..].iter().all(|&b| b == 0xBB));
	}

	#[test]
	fn test_asyncstream_empty() {
		let mut stream = pin!(AsyncStreamFn::new(async |_writer: AsyncStreamWriter| {}));
		let mut buf = Vec::new();
		let result = block_on(stream.read_to_end(&mut buf));

		assert!(result.is_ok());
		assert_eq!(result.unwrap(), 0);
		assert!(buf.is_empty());
	}

	#[test]
	fn test_asyncstream_write_string() {
		let mut stream = pin!(AsyncStreamFn::new(async |writer: AsyncStreamWriter| {
			writer.write_string(42u32).await;
			writer.write_string(" hello").await;
		}));
		let mut buf = Vec::new();
		block_on(stream.read_to_end(&mut buf)).unwrap();
		assert_eq!(buf, b"42 hello");
	}

	#[test]
	fn test_asyncstream_small_read_buffer() {
		let data = b"abcde";
		let mut stream = pin!(AsyncStreamFn::new(async |writer: AsyncStreamWriter| {
			writer.write(&data).await;
		}));

		let mut collected = Vec::new();
		block_on(async {
			let mut one_byte = [0u8; 1];
			loop {
				let n = stream.as_mut().read(&mut one_byte).await.unwrap();
				if n == 0 {
					break;
				}
				collected.push(one_byte[0]);
			}
		});

		assert_eq!(collected, data);
	}

	#[test]
	fn test_buffer_copy_to_empty() {
		let mut buf = Buffer::<16>::new();
		let mut target = [0u8; 16];
		assert_eq!(buf.copy_to(&mut target), 0);
		assert!(buf.is_empty);

		buf.copy_from(&target);
		assert_eq!(buf.copy_to(&mut []), 0);
		assert_eq!(buf.len(), 16);
	}

	#[test]
	fn test_buffer_copy_from_empty() {
		let mut buf = Buffer::<16>::new();
		assert_eq!(buf.copy_from(&[]), 0);
		assert!(buf.is_empty);
	}

	#[test]
	fn test_buffer_copy_from_simple() {
		let mut buf = Buffer::<16>::new();
		let data = b"Hello, world!";

		let n = buf.copy_from(data);
		assert_eq!(n, data.len());
		assert_eq!(buf.len(), data.len());
	}

	#[test]
	fn test_buffer_write_then_read() {
		let mut buf = Buffer::<16>::new();
		let data = b"Hello, world!";

		buf.copy_from(data);
		let mut target = [0u8; 16];
		let n = buf.copy_to(&mut target);

		assert_eq!(n, data.len());
		assert_eq!(&target[..n], data);
		assert_eq!(buf.len(), 0);
	}

	#[test]
	fn test_buffer_partial_copy_to() {
		let mut buf = Buffer::<32>::new();
		let data = b"abcdefghijklmnopqrstuvwxyz";
		buf.copy_from(data);

		let mut target = [0u8; 10];
		let n = buf.copy_to(&mut target);
		assert_eq!(n, 10);
		assert_eq!(&target, b"abcdefghij");
		assert!(buf.len() > 0);

		let mut rest = [0u8; 32];
		let n2 = buf.copy_to(&mut rest);
		assert_eq!(n2, data.len() - 10);
		assert_eq!(&rest[..n2], &data[10..]);
		assert_eq!(buf.len(), 0);
	}

	#[test]
	fn test_buffer_copy_to_full() {
		let mut buf = Buffer::<32>::new();
		buf.is_empty = false;
		buf.buffer.fill(0xAB);

		let mut target = [0u8; 32];
		let n = buf.copy_to(&mut target);
		assert_eq!(n, 32);
		assert!(target.iter().all(|&b| b == 0xAB));
		assert_eq!(buf.len(), 0);
	}

	#[test]
	fn test_buffer_copy_from_full() {
		let mut buf = Buffer::<32>::new();
		buf.is_empty = false;
		buf.head = 16;
		buf.tail = 16;

		let n = buf.copy_from(b"data");
		assert_eq!(n, 0);
		assert!(buf.len() != 0);
		assert_eq!(buf.tail, 16);
	}

	#[test]
	fn test_buffer_partial_copy_from() {
		let mut buf = Buffer::<32>::new();
		buf.head = 10;
		buf.tail = 20;
		buf.is_empty = false;

		// available = (cap - tail) + head = (32-20)+10 = 22
		let data = vec![0xBBu8; 32];
		let n = buf.copy_from(&data);
		assert_eq!(n, 22);
		assert_eq!(buf.tail, (20 + 22) % 32); // = 42 % 32 = 10
	}

	#[test]
	fn test_buffer_wrap_around() {
		let mut buf = Buffer::<32>::new();

		// Position the logical start near the end
		buf.head = 30;
		buf.tail = 30;
		buf.is_empty = true;

		// Write 20 bytes — wraps: buffer[30..32] + buffer[0..18]
		let data = b"ABCDEFGHIJKLMNOPQRST";
		buf.copy_from(data);

		assert_eq!(buf.tail, 18);
		assert_eq!(&buf.buffer[30..32], b"AB");
		assert_eq!(&buf.buffer[0..18], b"CDEFGHIJKLMNOPQRST");

		// Read back the 20 bytes — also wraps
		let mut target = [0u8; 20];
		let n = buf.copy_to(&mut target);
		assert_eq!(&target, b"ABCDEFGHIJKLMNOPQRST");
		assert_eq!(n, 20);
		assert_eq!(&target, data);
		assert_eq!(buf.len(), 0);
	}

	#[test]
	fn test_buffer_interleaved_cycles() {
		let mut buf = Buffer::<8>::new();

		// Each iteration writes 5 bytes then reads 5 bytes.
		// After the first iteration head/tail will have advanced, causing
		// subsequent writes to wrap around.
		for i in 0..8u8 {
			let src = [i; 5];
			let n = buf.copy_from(&src);
			assert_eq!(n, 5);
			assert_eq!(buf.len(), 5);

			let mut dst = [0u8; 5];
			let m = buf.copy_to(&mut dst);
			assert_eq!(m, 5);
			assert_eq!(dst, src);
			assert_eq!(buf.len(), 0);
			assert!(buf.is_empty);
		}
	}

	#[test]
	fn test_buffer_fill_exactly_full() {
		const CAP: usize = 8;
		let mut buf = Buffer::<CAP>::new();

		let src = [0xCCu8; CAP];
		let n = buf.copy_from(&src);
		assert_eq!(n, CAP);
		// head == tail and !is_empty → full
		assert!(!buf.is_empty);
		assert_eq!(buf.head, buf.tail);
		assert_eq!(buf.len(), CAP);

		// Trying to write more should return 0
		assert_eq!(buf.copy_from(b"extra"), 0);

		// Should be able to drain it fully
		let mut dst = [0u8; CAP];
		let m = buf.copy_to(&mut dst);
		assert_eq!(m, CAP);
		assert!(dst.iter().all(|&b| b == 0xCC));
		assert_eq!(buf.len(), 0);
	}
}
