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

use super::signal_gate::SignalGate;
use super::task::{Executor, Spawner};
use super::{Server, handle_socket};
use crate::tracing;
use async_net::{AsyncToSocketAddrs, TcpListener, TcpStream};
#[cfg(feature = "signals")]
use futures_lite::future::FutureExt;
use std::io::ErrorKind;
#[cfg(feature = "signals")]
use std::time::Duration;
use std::{net::SocketAddr, num::NonZeroUsize, thread};

#[derive(Default)]
#[cfg_attr(docsrs, doc(cfg(feature = "mt")))]
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
/// # Example
/// ```no_run
/// use moonbeam::{Server, Request, Response, Spawner, serve_multi, ThreadCount};
///
/// struct MyServer;
///
/// impl Server for MyServer {
///     async fn route<'s: 'e, 'e>(
///         &'s self,
///         _req: Request<'_, '_>,
///         _spawner: Spawner<'e>,
///     ) -> Response {
///         Response::ok()
///     }
/// }
///
/// serve_multi(
///     "127.0.0.1:8080",
///     ThreadCount::Default,
///     || MyServer, // Factory creates a new instance per thread
/// );
/// ```
#[cfg_attr(docsrs, doc(cfg(feature = "mt")))]
pub fn serve_multi<F, T: Server>(addr: impl AsyncToSocketAddrs, num_threads: ThreadCount, server: F)
where
	F: FnOnce() -> T + Send + Clone,
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
		let (main_done_shutdown, worker_force_shutdown, worker_done_shutdown, all_workers_shutdown) =
			make_signals();

		for _i in 0..num_threads {
			let server = server.clone();
			let worker_done_shutdown = worker_done_shutdown.clone();
			let worker_force_shutdown = worker_force_shutdown.clone();
			let recv = recv.clone();
			scope.spawn(move || {
				let server = server();
				let executor = Executor::new();
				let spawner = executor.spawner();

				let _span = tracing::trace_span!("thread", id = _i).entered();
				async_io::block_on(executor.run(async {
					while let Ok((socket, addr)) = recv.recv_async().await {
						let _ = socket.set_nodelay(true);
						spawner.spawn(handle_socket(socket, addr, &server, spawner));
					}

					tracing::debug!(id = _i, "Worker thread shutting down");

					#[cfg(feature = "signals")]
					spawner
						.wait_until_empty(Duration::from_secs(60))
						.or(async {
							let _ = worker_force_shutdown.recv_async().await;
						})
						.await;
				}));

				tracing::debug!(id = _i, "Worker thread shut down");
				drop(worker_force_shutdown);
				drop(worker_done_shutdown);
			});
		}
		drop(worker_force_shutdown);
		drop(worker_done_shutdown);

		let executor = Executor::new();
		let spawner = executor.spawner();
		#[cfg(feature = "signals")]
		spawner.spawn(async move {
			let _ = all_workers_shutdown.recv_async().await;
			tracing::debug!("All worker threads shut down");
		});
		#[cfg(not(feature = "signals"))]
		drop(all_workers_shutdown);

		async_io::block_on(executor.run(async move {
			let listener = TcpListener::bind(addr)
				.await
				.expect("Failed to bind to socket");

			accept_loop(listener, send, spawner).await;
		}));

		tracing::debug!("Server shut down");
		drop(main_done_shutdown);
	});
}

#[cfg(feature = "signals")]
fn make_signals() -> (
	flume::Sender<()>,
	flume::Receiver<()>,
	flume::Sender<()>,
	flume::Receiver<()>,
) {
	let (main_done_shutdown, worker_force_shutdown) = flume::bounded::<()>(0);
	let (worker_done_shutdown, all_workers_shutdown) = flume::bounded::<()>(0);
	(
		main_done_shutdown,
		worker_force_shutdown,
		worker_done_shutdown,
		all_workers_shutdown,
	)
}

#[cfg(not(feature = "signals"))]
#[derive(Clone)]
struct DummySenderReceiver;

#[cfg(not(feature = "signals"))]
impl Drop for DummySenderReceiver {
	fn drop(&mut self) {}
}

#[cfg(not(feature = "signals"))]
fn make_signals() -> (
	DummySenderReceiver,
	DummySenderReceiver,
	DummySenderReceiver,
	DummySenderReceiver,
) {
	(
		DummySenderReceiver,
		DummySenderReceiver,
		DummySenderReceiver,
		DummySenderReceiver,
	)
}

async fn accept_loop(
	listener: TcpListener,
	sender: flume::Sender<(TcpStream, SocketAddr)>,
	spawner: Spawner<'_>,
) {
	let mut gate = SignalGate::new();

	loop {
		match gate.or_signal(listener.accept()).await {
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

	gate.wait_for_shutdown(spawner).await;
}
