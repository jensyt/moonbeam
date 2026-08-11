//! Task management and spawning.
//!
//! Use the [`Spawner`] instance provided on [`Server::route`](super::Server::route) callbacks to
//! spawn tasks. Since Moonbeam follows a share nothing approach to threading, these tasks will be
//! queued to run on the same thread that spawned them.
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
use std::{marker::PhantomPinned, pin::Pin, time::Duration};

#[cfg(feature = "signals")]
use crate::server::task_tracker::TaskTracker;
use async_executor::LocalExecutor;

/// A handle for spawning tasks on an [`Executor`].
///
/// Spawners can be cheaply cloned and passed around. Tasks spawned via a `Spawner` are owned by the
/// parent `Executor` and will be dropped when the executor is dropped.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Spawner<'exec> {
	ex: *const Executor<'exec>,
}

impl<'exec> Spawner<'exec> {
	/// Spawns a task onto the executor.
	///
	/// The task is detached and will run to completion (or until the executor is dropped).
	pub fn spawn<T: 'exec>(&self, future: impl Future<Output = T> + 'exec) {
		// SAFETY:
		// Callers of Executor::spawner are responsible for ensuring the executor outlives this
		// spawner.
		unsafe {
			#[cfg(feature = "signals")]
			let future = {
				let guard = (*self.ex).tracker.track();
				async move {
					let _guard = guard;
					future.await
				}
			};
			(*self.ex).executor.spawn(future).detach();
		}
	}

	#[allow(unused)]
	pub(super) async fn wait_until_empty(self, timeout: Duration) {
		// SAFETY:
		// Callers of Executor::spawner are responsible for ensuring the executor outlives this
		// spawner.
		#[cfg(feature = "signals")]
		unsafe {
			(*self.ex).tracker.wait_until_empty(timeout).await
		}
	}
}

/// A local executor for running asynchronous tasks.
///
/// This is a wrapper around [`LocalExecutor`] that provides safe task tracking and spawning via
/// [`Spawner`].
pub(super) struct Executor<'exec> {
	executor: LocalExecutor<'exec>,
	#[cfg(feature = "signals")]
	tracker: TaskTracker,
	_pin: PhantomPinned,
}

impl<'exec> Executor<'exec> {
	/// Creates a new `Executor`.
	pub fn new() -> Self {
		Self::default()
	}

	/// Returns a [`Spawner`] for this executor.
	///
	/// # Safety
	///
	/// Callers of this function must ensure that `self` outlives the returned `Spawner`.
	pub unsafe fn spawner(self: Pin<&Self>) -> Spawner<'exec> {
		Spawner { ex: self.get_ref() }
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
			_pin: PhantomPinned,
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

/// Utilities for testing servers and background tasks.
pub mod testing {
	use super::*;
	use crate::http::{Request, Response};
	use crate::server::Server;
	use futures_lite::future::block_on;
	use std::pin::pin;

	/// A handle allowing tests to manually advance the task executor.
	pub struct Tick<'a, 'exec> {
		ex: Pin<&'a mut Executor<'exec>>,
	}

	impl<'a, 'exec> Tick<'a, 'exec> {
		/// Attempts to advance the executor by ticking a single ready task.
		///
		/// Returns `true` if a task was ticked, or `false` if no tasks were ready.
		pub fn try_tick(&self) -> bool {
			self.ex.try_tick()
		}
	}

	/// Executes a request against a server for testing, allowing inspection of the response
	/// and manual executor ticking.
	///
	/// # Example
	///
	/// ```
	/// use moonbeam::server::task::testing::execute;
	/// use moonbeam::{Request, Response, Spawner, route, router};
	/// use std::cell::Cell;
	///
	/// struct TestState {
	///     value: Cell<i32>,
	/// }
	///
	/// #[route]
	/// async fn spawn_closure(state: &TestState, spawner: Spawner) -> Response {
	///     spawner.spawn(async {
	///         state.value.update(|v| v + 1);
	///     });
	///     Response::ok()
	/// }
	///
	/// router! {
	///     TestRouter<TestState> {
	///         get("/closure") => spawn_closure,
	///     }
	/// }
	///
	/// let state = TestState {
	///     value: Cell::new(42),
	/// };
	/// let router = TestRouter::new(state);
	///
	/// let req = Request::new("GET", "/closure", &[], &[]);
	/// execute(
	///     &router,
	///     req,
	///     |res| {
	///         assert_eq!(res.status, 200);
	///     },
	///     |tick| {
	///         assert_eq!(router.0.value.get(), 42);
	///         assert_eq!(tick.try_tick(), true);
	///         assert_eq!(router.0.value.get(), 43);
	///         assert_eq!(tick.try_tick(), false);
	///     },
	/// );
	/// ```
	pub fn execute(
		server: &impl Server,
		req: Request,
		eval: impl FnOnce(Response),
		tick: impl FnOnce(Tick),
	) {
		let executor = pin!(Executor::new());
		// SAFETY: Spawner does not outlive executor because it can't escape this scope
		let res = block_on(server.route(req, unsafe { executor.as_ref().spawner() }));
		eval(res);
		tick(Tick { ex: executor });
	}
}
