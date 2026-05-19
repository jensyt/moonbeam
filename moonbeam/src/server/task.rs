//! Task management and spawning.
//!
//! Use the [`Spawner`] instance provided on [`Server::serve`] callbacks to spawn tasks. Since
//! Moonbeam follows a share nothing approach to threading, these tasks will be queued to run on the
//! same thread that spawned them.
//!
//! # Examples
//! /// ```no_run
/// use moonbeam::{Server, Request, Response, Spawner, serve};
///
/// struct MyServer;
///
/// impl Server for MyServer {
///     async fn route<'s: 'e, 'e>(&'s self, _req: Request, spawner: Spawner<'e>) -> Response {
///         spawner.spawn(async {
///             // Do something interesting here after the request is processed
///         });
///         Response::ok()
///     }
/// }
///
/// serve("127.0.0.1:8080", MyServer);
/// ```
use std::cell::UnsafeCell;

#[cfg(feature = "signals")]
use crate::server::task_tracker::TaskTracker;
use crate::tracing;
use async_executor::LocalExecutor;

/// A handle for spawning tasks on an [`Executor`].
///
/// Spawners can be cheaply cloned and passed around. Tasks spawned via a `Spawner` are owned by the
/// parent `Executor` and will be dropped when the executor is dropped.
#[derive(Clone, Copy)]
pub struct Spawner<'e> {
	ex: *const Executor<'e>,
	alive: *mut bool,
}

impl<'e> Spawner<'e> {
	/// Spawns a task onto the executor.
	///
	/// The task is detached and will run to completion (or until the executor is dropped).
	pub fn spawn<T: 'e>(&self, future: impl Future<Output = T> + 'e) {
		// SAFETY:
		// Tasks are owned by the LocalExecutor. They can only execute or be dropped while the
		// Executor is valid in memory, so derefencing the pointers will always be valid here.
		// The alive flag is toggled in the Executor's drop method, so as long as it returns true
		// the executor is valid and the ex pointer is safe to dereference and spawn tasks.
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

/// A local executor for running asynchronous tasks.
///
/// This is a wrapper around [`LocalExecutor`] that provides safe task tracking and lifetime-aware
/// spawning via [`Spawner`].
///
/// This type is primarily exposed for testing and debugging purposes. Moonbeam manages the
/// lifecycle of executors internally, so users should not need to interact with this type directly.
pub struct Executor<'e> {
	executor: LocalExecutor<'e>,
	#[cfg(feature = "signals")]
	tracker: TaskTracker,
	alive: UnsafeCell<bool>,
}

impl<'e> Executor<'e> {
	/// Creates a new `Executor`.
	pub fn new() -> Self {
		Self {
			executor: LocalExecutor::new(),
			#[cfg(feature = "signals")]
			tracker: TaskTracker::new(),
			alive: UnsafeCell::new(true),
		}
	}

	/// Returns a [`Spawner`] for this executor.
	pub fn spawner(&self) -> Spawner<'e> {
		Spawner {
			ex: self,
			alive: self.alive.get(),
		}
	}

	/// Runs the executor until the given future completes.
	#[inline(always)]
	pub fn run<T>(&self, future: impl Future<Output = T>) -> impl Future<Output = T> {
		self.executor.run(future)
	}

	/// Tries to advance the executor by one tick.
	///
	/// Returns `true` if any task was run.
	#[inline(always)]
	pub fn try_tick(&self) -> bool {
		self.executor.try_tick()
	}
}

impl<'e> Drop for Executor<'e> {
	fn drop(&mut self) {
		// SAFETY:
		// `self.alive` can be safely dereferenced and written to before drop is completed
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
