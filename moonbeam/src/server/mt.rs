//! # Multi-Threaded Server (Isolates)
//!
//! This module provides the multi-threaded server implementation for Moonbeam,
//! enabled via the `mt` feature.
//!
//! ## Share-Nothing Multi-Threading
//!
//! Unlike most multi-threaded servers that use shared state protected by locks, Moonbeam uses a
//! "share-nothing" model. Instead of sharing a single server state across all threads, Moonbeam
//! spawns a number of independent worker threads, each with its own local `Server` instance and
//! `LocalExecutor`.
//!
//! ### Key Benefits
//!
//! - **No Locks**: Each thread has its own state copy, eliminating `Mutex` or `RwLock` contention.
//! - **Interior Mutability**: Handlers on each thread can still use `Cell` and `RefCell` for
//!   per-thread state modification.
//! - **Simplified API**: You don't need `Arc` or `Send`/`Sync` bounds for your server state.
//!
//! ### Connection Distribution
//!
//! The main thread runs an acceptance loop that binds to the specified port. When a connection is
//! accepted, it is sent over an asynchronous channel to one of the worker threads, which then
//! handles it locally.

use super::task::{get_local_executor, new_local_task};
use super::{Server, handle_socket};
use crate::tracing;
use async_net::{AsyncToSocketAddrs, TcpListener, TcpStream};
use std::io::ErrorKind;
use std::{net::SocketAddr, num::NonZeroUsize, thread};

#[derive(Default)]
/// Specifies the number of worker threads to spawn for the multi-threaded server.
pub enum ThreadCount {
	/// Uses the number of available CPU cores (via `std::thread::available_parallelism`), or 1 if
	/// it cannot be determined.
	#[default]
	Default,
	/// Explicitly specifies the number of threads to spawn.
	Count(usize),
}

/// Starts the server across multiple threads using a "share-nothing" architecture.
///
/// This function blocks the current thread, spawns the requested number of worker threads,
/// and distributes incoming connections across them. To avoid complex synchronization, Moonbeam
/// creates a separate instance of your server state for each thread using the `server` factory
/// closure.
///
/// Note that server instances are leaked and should typically be cleaned up by providing a
/// `cleanup` closure.
///
/// # Example
/// ```no_run
/// use moonbeam::{Server, Request, Response, serve_multi, ThreadCount};
/// use std::future::Future;
///
/// struct MyServer;
///
/// impl Server for MyServer {
///     fn route(&'static self, _req: Request) -> impl Future<Output = Response> {
///         async { Response::ok() }
///     }
/// }
///
/// serve_multi(
///     "127.0.0.1:8080",
///     ThreadCount::Default,
///     || MyServer, // Factory creates a new instance per thread
///     |s| {
///         // Safety: application is shutting down, so it is safe to clean up resources
///         unsafe { s.destroy(); }
///     }
/// );
/// ```
#[inline(always)]
pub fn serve_multi<F, C, T: Server>(
	addr: impl AsyncToSocketAddrs,
	num_threads: ThreadCount,
	server: F,
	cleanup: C,
) where
	F: FnOnce() -> T + Send + Clone,
	C: FnOnce(&'static T) + Send + Clone,
{
	serve_multi_impl(addr, num_threads, server, cleanup);
}

#[cfg(feature = "signals")]
pub fn serve_multi_impl<F, C, T: Server>(
	addr: impl AsyncToSocketAddrs,
	num_threads: ThreadCount,
	server: F,
	cleanup: C,
) where
	F: FnOnce() -> T + Send + Clone,
	C: FnOnce(&'static T) + Send + Clone,
{
	use super::task_tracker::get_local_tracker;
	use futures_lite::FutureExt;
	use std::time::Duration;

	let _span = tracing::trace_span!("thread", id = "main").entered();
	let num_threads = match num_threads {
		ThreadCount::Default => thread::available_parallelism()
			.map(NonZeroUsize::get)
			.unwrap_or(1),
		ThreadCount::Count(n) => n,
	};
	tracing::debug!(num_threads, "Starting worker threads");

	thread::scope(|scope| {
		let (send, recv) = flume::bounded::<(TcpStream, SocketAddr)>(num_threads * 100);
		let (main_done_shutdown, worker_force_shutdown) = flume::bounded::<()>(0);
		let (worker_done_shutdown, all_workers_shutdown) = flume::bounded::<()>(0);

		for _i in 0..num_threads {
			let server = server.clone();
			let cleanup = cleanup.clone();
			let worker_done_shutdown = worker_done_shutdown.clone();
			let worker_force_shutdown = worker_force_shutdown.clone();
			let recv = recv.clone();
			scope.spawn(move || {
				let server = Box::leak(Box::new(server()));

				let _span = tracing::trace_span!("thread", id = _i).entered();
				async_io::block_on(get_local_executor().run(async {
					while let Ok((socket, addr)) = recv.recv_async().await {
						let _ = socket.set_nodelay(true);
						new_local_task(handle_socket(socket, addr, server));
					}

					tracing::debug!(id = _i, "Worker thread shutting down");

					get_local_tracker()
						.wait_until_empty(Duration::from_secs(60))
						.or(async {
							let _ = worker_force_shutdown.recv_async().await;
						})
						.await;
				}));

				cleanup(server);

				tracing::debug!(id = _i, "Worker thread shut down");
				drop(worker_done_shutdown);
			});
		}
		drop(worker_force_shutdown);
		drop(worker_done_shutdown);

		new_local_task(async move {
			let _ = all_workers_shutdown.recv_async().await;
			tracing::debug!("All worker threads shut down");
		});

		async_io::block_on(get_local_executor().run(async move {
			let listener = TcpListener::bind(addr)
				.await
				.expect("Failed to bind to socket");

			accept_loop(listener, send).await;
		}));

		tracing::debug!("Server shut down");
		drop(main_done_shutdown);
	});
}

#[cfg(feature = "signals")]
async fn accept_loop(listener: TcpListener, sender: flume::Sender<(TcpStream, SocketAddr)>) {
	use super::task_tracker::get_local_tracker;
	use async_signal::{Signal, Signals};
	use futures_lite::{FutureExt, StreamExt};
	use std::{io::Error, time::Duration};

	let mut signals =
		Signals::new([Signal::Int, Signal::Term]).expect("Failed to create signal handler");

	loop {
		let signal_err = async {
			let signal = signals.next().await;
			Err(Error::new(
				ErrorKind::Interrupted,
				format!("Signal: {signal:?}"),
			))
		};

		match listener.accept().or(signal_err).await {
			Ok(v) => {
				if let Err(_error) = sender.send_async(v).await {
					tracing::error!(?_error, "Failed to send socket to thread, shutting down");
					break;
				}
			}
			Err(error) => {
				if error.kind() == ErrorKind::Interrupted {
					tracing::debug!(?error, "Got signal to shut down");
				} else {
					tracing::error!(?error, "Failed to accept connection, shutting down");
				}
				break;
			}
		}
	}
	drop(sender);

	let wait_for_tasks = get_local_tracker().wait_until_empty(Duration::from_secs(60));

	let force_shutdown = async {
		if let Some(_signal) = signals.next().await {
			tracing::warn!(?_signal, "Received signal, forcing shutdown");
		}
	};

	wait_for_tasks.or(force_shutdown).await;
}

#[cfg(not(feature = "signals"))]
pub fn serve_multi_impl<F, C, T: Server>(
	addr: impl AsyncToSocketAddrs,
	num_threads: ThreadCount,
	server: F,
	cleanup: C,
) where
	F: FnOnce() -> T + Send + Clone,
	C: FnOnce(&'static T) + Send + Clone,
{
	let _span = tracing::trace_span!("thread", id = "main").entered();
	let num_threads = match num_threads {
		ThreadCount::Default => thread::available_parallelism()
			.map(NonZeroUsize::get)
			.unwrap_or(1),
		ThreadCount::Count(n) => n,
	};
	tracing::debug!(num_threads, "Starting worker threads");

	thread::scope(|scope| {
		let (send, recv) = flume::bounded::<(TcpStream, SocketAddr)>(num_threads * 100);

		for _i in 0..num_threads {
			let server = server.clone();
			let cleanup = cleanup.clone();
			let recv = recv.clone();
			scope.spawn(move || {
				let server = Box::leak(Box::new(server()));

				let _span = tracing::trace_span!("thread", id = _i).entered();
				async_io::block_on(get_local_executor().run(async {
					while let Ok((socket, addr)) = recv.recv_async().await {
						let _ = socket.set_nodelay(true);
						new_local_task(handle_socket(socket, addr, server));
					}
				}));

				cleanup(server);
			});
		}

		async_io::block_on(get_local_executor().run(async move {
			let listener = TcpListener::bind(addr)
				.await
				.expect("Failed to bind to socket");

			accept_loop(listener, send).await;
		}));
	});
}

#[cfg(not(feature = "signals"))]
async fn accept_loop(listener: TcpListener, sender: flume::Sender<(TcpStream, SocketAddr)>) {
	loop {
		match listener.accept().await {
			Ok(v) => {
				if let Err(_error) = sender.send_async(v).await {
					tracing::error!(?_error, "Failed to send socket to thread, shutting down");
					break;
				}
			}
			Err(error) => {
				if error.kind() == ErrorKind::Interrupted {
					tracing::debug!(?error, "Got signal to shut down");
				} else {
					tracing::error!(?error, "Failed to accept connection, shutting down");
				}
				break;
			}
		}
	}
}
