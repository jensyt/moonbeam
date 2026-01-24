use super::task::{get_local_executor, new_local_task};
use super::{Server, handle_socket};
use crate::tracing;
use async_net::{AsyncToSocketAddrs, TcpListener, TcpStream};
use std::io::ErrorKind;
use std::{net::SocketAddr, num::NonZeroUsize, thread};

#[derive(Default)]
pub enum ThreadCount {
	#[default]
	Default,
	Count(usize),
}

/// Starts the server on the specified address.
///
/// This function blocks the current thread and runs the server loop. It takes a factory function to
/// create server instances and cleanup function to destroy them (if needed). Note that server
/// instances are leaked and should typically be cleaned up.
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
/// serve_multi("127.0.0.1:8080", ThreadCount::Default, || MyServer, |s| {
///     // Safety: application is shutting down, so it is safe to clean up resources
///     unsafe { s.destroy(); }
/// });
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
