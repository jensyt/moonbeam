use std::{cell::UnsafeCell, marker::PhantomData};

pub(super) fn get_local_bufpool() -> &'static BufPool {
	thread_local! {
		static POOL: BufPool = const { BufPool::new() };
	}

	POOL.with(|pool| {
		// SAFETY: The thread-local executor lives for the entire thread lifetime
		unsafe { std::mem::transmute(pool) }
	})
}

pub(super) const BUFSIZE: usize = 16 * 1024;

pub(super) struct Buffer {
	data: Vec<u8>,
}

impl Buffer {
	fn new(v: Vec<u8>) -> Self {
		Self { data: v }
	}

	// pub fn get(&self) -> &[u8] {
	// 	&self.data
	// }

	pub fn get_mut(&mut self) -> &mut [u8] {
		&mut self.data
	}
}

impl Default for Buffer {
	fn default() -> Self {
		Self {
			data: vec![0; BUFSIZE],
		}
	}
}

impl Drop for Buffer {
	fn drop(&mut self) {
		get_local_bufpool().give(std::mem::take(&mut self.data));
	}
}

pub(super) struct BufPool {
	buffers: UnsafeCell<Vec<Vec<u8>>>,
	min_size: UnsafeCell<usize>,
	// Make this type !Send since it is meant to be thread-local only
	_phantom: PhantomData<*const ()>,
}

impl BufPool {
	pub const fn new() -> Self {
		Self {
			buffers: UnsafeCell::new(Vec::new()),
			min_size: UnsafeCell::new(0),
			_phantom: PhantomData,
		}
	}

	pub fn get(&self) -> Buffer {
		let buf = unsafe { &mut *self.buffers.get() };
		unsafe {
			let size = self.min_size.get();
			if *size > buf.len() {
				*size = buf.len();
			}
		}
		buf.pop().map(Buffer::new).unwrap_or(Buffer::default())
	}

	pub fn give(&self, buf: Vec<u8>) {
		unsafe { (*self.buffers.get()).push(buf) };
	}

	pub fn reset(&self) {
		let size = unsafe { &mut *self.min_size.get() };
		let buf = unsafe { &mut *self.buffers.get() };

		// Always keep at least 1 buffer around
		let newsize = std::cmp::max(1, buf.len().saturating_sub(*size));
		buf.truncate(newsize);

		*size = buf.len();
	}
}
