//! Task management and spawning.
//!
//! Use the [`Spawner`] instance provided on [`Server::serve`] callbacks to spawn tasks. Since
//! Moonbeam follows a share nothing approach to threading, these tasks will be queued to run on the
//! same thread that spawned them.
//!
//! # Examples
//! ```no_run
//! use moonbeam::{Server, Request, Response, Spawner, serve};
//!
//! struct MyServer;
//!
//! impl Server for MyServer {
//!     async fn route<'e: 'r, 'r>(
//!         &'e self,
//!         _req: Request<'r, 'r>,
//!         spawner: Spawner<'e>,
//!     ) -> Response<'r>
//!     {
//!         spawner.spawn(async {
//!             // Do something interesting here after the request is processed
//!         });
//!         Response::ok()
//!     }
//! }
//!
//! serve("127.0.0.1:8080", || MyServer);
//! ```
use std::{cell::UnsafeCell, marker::PhantomPinned, pin::Pin, time::Duration};

#[cfg(feature = "signals")]
use crate::server::task_tracker::TaskTracker;
use crate::tracing;
use async_executor::LocalExecutor;

/// A handle for spawning tasks on an [`Executor`].
///
/// Spawners can be cheaply cloned and passed around. Tasks spawned via a `Spawner` are owned by the
/// parent `Executor` and will be dropped when the executor is dropped.
#[derive(Clone, Copy)]
pub struct Spawner<'exec> {
	ex: *const Executor<'exec>,
	alive: *mut bool,
}

impl<'exec> Spawner<'exec> {
	/// Spawns a task onto the executor.
	///
	/// The task is detached and will run to completion (or until the executor is dropped).
	pub fn spawn<T: 'exec>(&self, future: impl Future<Output = T> + 'exec) {
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

	#[allow(unused)]
	pub(super) async fn wait_until_empty(self, timeout: Duration) {
		// SAFETY:
		// Tasks are owned by the LocalExecutor. They can only execute or be dropped while the
		// Executor is valid in memory, so derefencing the pointers will always be valid here.
		// The alive flag is toggled in the Executor's drop method, so as long as it returns true
		// the executor is valid and the ex pointer is safe to dereference and use the tracker.
		#[cfg(feature = "signals")]
		unsafe {
			if *self.alive {
				(*self.ex).tracker.wait_until_empty(timeout).await
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
pub struct Executor<'exec> {
	executor: LocalExecutor<'exec>,
	#[cfg(feature = "signals")]
	tracker: TaskTracker,
	alive: UnsafeCell<bool>,
	_pin: PhantomPinned,
}

impl<'exec> Executor<'exec> {
	/// Creates a new `Executor`.
	pub fn new() -> Self {
		Self::default()
	}

	/// Returns a [`Spawner`] for this executor.
	pub fn spawner(self: Pin<&Self>) -> Spawner<'exec> {
		Spawner {
			ex: self.get_ref(),
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

impl<'exec> Default for Executor<'exec> {
	fn default() -> Self {
		Self {
			executor: LocalExecutor::new(),
			#[cfg(feature = "signals")]
			tracker: TaskTracker::new(),
			alive: UnsafeCell::new(true),
			_pin: PhantomPinned,
		}
	}
}

impl<'exec> Drop for Executor<'exec> {
	fn drop(&mut self) {
		// SAFETY:
		// `self.alive` can be safely dereferenced and written to before drop is completed
		unsafe {
			*self.alive.get() = false;
		}
	}
}

/// Spawns a task onto the executor, instrumenting it with a child span that inherits the current
/// tracing context.
///
/// Under the hood, this creates a span named `"spawned_task"` with the tag `task = "name"`. You can
/// optionally supply additional key-value properties to log metadata.
///
/// If the `tracing` feature is disabled, this compiles down to a direct call to
/// `spawner.spawn(future)` with no runtime or allocation overhead.
///
/// # Examples
/// ```
/// # use moonbeam::{Spawner, spawn_with_span};
/// # async fn example(spawner: Spawner<'_>) {
/// // Simple spawn
/// spawn_with_span!(spawner, "send_email", async { /* ... */ });
///
/// // Spawn with custom metadata fields
/// spawn_with_span!(spawner, "db_cleanup", async { /* ... */ }, user_id = 42, count = 10);
/// # }
/// ```
#[macro_export]
macro_rules! spawn_with_span {
	// With key-value fields: spawn_with_span!(spawner, "name", future, key1 = val1, key2 = val2, ...)
	($spawner:expr, $name:expr, $future:expr, $($field:ident = $val:expr),+ $(,)?) => {
		{
			use $crate::tracing::Instrument;
			let span = $crate::tracing::info_span!("spawned_task", task = $name, $($field = $val),*);
			$spawner.spawn($future.instrument(span));
		}
	};

	// Without key-value fields: spawn_with_span!(spawner, "name", future)
	($spawner:expr, $name:expr, $future:expr) => {
		{
			use $crate::tracing::Instrument;
			let span = $crate::tracing::info_span!("spawned_task", task = $name);
			$spawner.spawn($future.instrument(span));
		}
	};
}
