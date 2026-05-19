//! Task creation on the local executor.
//!
//! Use [`new_local_task`] to spawn a detached task that will run to completion as long as the
//! local executor is running.
use std::cell::UnsafeCell;

#[cfg(feature = "signals")]
use crate::server::task_tracker::TaskTracker;
use crate::tracing;
use async_executor::LocalExecutor;

#[derive(Clone, Copy)]
pub struct Spawner<'e> {
	ex: *const Executor<'e>,
	alive: *mut bool,
}

impl<'e> Spawner<'e> {
	pub fn spawn<T: 'e>(&self, future: impl Future<Output = T> + 'e) {
		// SAFETY:
		// Tasks are owned by the LocalExecutor. They can only execute or be dropped while the
		// Executor is valid in memory, so derefencing the pointers will always be valid here.
		unsafe {
			if *self.alive {
				#[cfg(feature = "signals")]
				let future = {
					let guard = (*self.ex).tracker.track();
					async move {
						let _guard = guard;
						future.await
					}
				};
				(*self.ex).executor.spawn(future).detach();
			} else {
				tracing::warn!("Attempting to spawn a task on an inactive executor");
			}
		}
	}
}

pub struct Executor<'e> {
	executor: LocalExecutor<'e>,
	#[cfg(feature = "signals")]
	tracker: TaskTracker,
	alive: UnsafeCell<bool>,
}

impl<'e> Executor<'e> {
	pub fn new() -> Self {
		Self {
			executor: LocalExecutor::new(),
			#[cfg(feature = "signals")]
			tracker: TaskTracker::new(),
			alive: UnsafeCell::new(true),
		}
	}

	pub fn spawner(&self) -> Spawner<'e> {
		Spawner {
			ex: self,
			alive: self.alive.get(),
		}
	}

	#[inline(always)]
	pub fn run<T>(&self, future: impl Future<Output = T>) -> impl Future<Output = T> {
		self.executor.run(future)
	}

	#[inline(always)]
	pub fn try_tick(&self) -> bool {
		self.executor.try_tick()
	}
}

impl<'e> Drop for Executor<'e> {
	fn drop(&mut self) {
		// SAFETY:
		// `self.alive` can be safely written to before drop is completed
		unsafe {
			*self.alive.get() = false;
		}
	}
}

pub(super) fn get_local_executor() -> &'static LocalExecutor<'static> {
	thread_local! {
		static EXECUTOR: LocalExecutor = const { LocalExecutor::new() };
	}

	EXECUTOR.with(|ex| {
		// SAFETY: The thread-local executor lives for the entire thread lifetime
		unsafe { std::mem::transmute(ex) }
	})
}

/// Spawns a new task on the local executor.
///
/// This function spawns a task that runs on the current thread's executor.
/// The task is detached and will run concurrently with other tasks.
///
/// If the `signals` feature is enabled, this task is tracked for graceful shutdown.
#[cfg(feature = "signals")]
pub fn new_local_task<T: 'static>(future: impl Future<Output = T> + 'static) {
	use super::task_tracker::get_local_tracker;

	let guard = get_local_tracker().track();
	get_local_executor()
		.spawn(async {
			let _guard = guard;
			future.await
		})
		.detach();
}

/// Spawns a new task on the local executor.
///
/// This function spawns a task that runs on the current thread's executor.
/// The task is detached and will run concurrently with other tasks.
#[cfg(not(feature = "signals"))]
pub fn new_local_task<T: 'static>(future: impl Future<Output = T> + 'static) {
	get_local_executor().spawn(future).detach();
}
