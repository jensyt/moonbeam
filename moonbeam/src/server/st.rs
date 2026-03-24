//! # Single-Threaded Server
//!
//! This module implements the single-threaded server runtime for Moonbeam.
//!
//! The `serve` function provides a convenient way to run a Moonbeam server using a `LocalExecutor`
//! on the main thread. This allows handlers to access non-`Send`/`Sync` state safely.
//!
//! ## Execution Model
//!
//! - Incoming connections are accepted in a loop.
//! - Each connection is handled by a new local task spawned on the `LocalExecutor`.
//! - All tasks run on the same thread, meaning no true parallelism between handlers.
//! - I/O is asynchronous, so multiple connections can be handled concurrently.
//! - If a handler performs blocking I/O or heavy CPU work, it will block all other connections.

use super::task::{get_local_executor, new_local_task};
use super::{Server, handle_socket};
use crate::tracing;
use async_net::{AsyncToSocketAddrs, TcpListener};

/// Starts the server on the specified address using a single-threaded local executor.
///
/// This function blocks the current thread and runs the server loop, driving all incoming async
/// connections concurrently on a single thread. Because it uses a `LocalExecutor`, you can safely
/// use non-Send/Sync types like `Cell` and `RefCell` in your server state.
///
/// It takes ownership of the server instance and leaks it to create a static reference, which is
/// required to satisfy the executor's lifetime constraints.
///
/// # Warning
/// CPU-heavy operations or synchronous blocking I/O inside your handlers will block the entire
/// server. Use `blocking::unblock` for these tasks.
///
/// # Example
/// ```no_run
/// use moonbeam::{Server, Request, Response, serve};
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
/// serve("127.0.0.1:8080", MyServer);
/// ```
pub fn serve<T: Server>(addr: impl AsyncToSocketAddrs, server: T) -> &'static T {
	let static_server = Box::leak(Box::new(server));
	async_io::block_on(get_local_executor().run(async {
		let listener = TcpListener::bind(addr)
			.await
			.expect("Failed to bind to socket");
		accept_loop(listener, static_server).await;
	}));
	static_server
}

#[cfg(feature = "signals")]
async fn accept_loop<T: Server>(listener: TcpListener, server: &'static T) {
	use super::task_tracker::get_local_tracker;
	use async_signal::{Signal, Signals};
	use futures_lite::{FutureExt, StreamExt};
	use std::{
		io::{Error, ErrorKind},
		time::Duration,
	};

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
			Ok((socket, addr)) => {
				let _ = socket.set_nodelay(true);
				new_local_task(handle_socket(socket, addr, server));
			}
			Err(err) => {
				if err.kind() == ErrorKind::Interrupted {
					tracing::debug!(?err, "Got signal to shut down");
				} else {
					tracing::error!(?err, "Failed to accept connection, shutting down");
				}
				break;
			}
		}
	}

	let wait_for_tasks = get_local_tracker().wait_until_empty(Duration::from_secs(60));

	let force_shutdown = async {
		if let Some(_signal) = signals.next().await {
			tracing::warn!(?_signal, "Received signal, forcing shutdown");
		}
	};

	wait_for_tasks.or(force_shutdown).await;
}

#[cfg(not(feature = "signals"))]
#[allow(clippy::while_let_loop)]
async fn accept_loop<T: Server>(listener: TcpListener, server: &'static T) {
	loop {
		match listener.accept().await {
			Ok((socket, addr)) => {
				let _ = socket.set_nodelay(true);
				new_local_task(handle_socket(socket, addr, server));
			}
			Err(_err) => {
				tracing::error!(?_err, "Failed to accept connection, shutting down");
				break;
			}
		}
	}
}

#[cfg(test)]
mod test {
	use super::*;
	use crate::http::{Body, Request, Response};
	use futures_lite::{AsyncReadExt, AsyncWriteExt};

	struct MockServer;
	impl Server for MockServer {
		async fn route(&'static self, req: Request<'_, '_>) -> Response {
			if req.path == "/error" {
				panic!("forced panic");
			}
			Response::ok().with_body(format!("Hello {}", req.path), Body::DEFAULT_CONTENT_TYPE)
		}
	}

	#[test]
	fn test_serve() {
		use async_net::TcpListener;
		use std::time::Duration;

		// Pick a random port by binding to 0 and getting the address
		let addr = futures_lite::future::block_on(async {
			let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
			listener.local_addr().unwrap()
		});

		let server = MockServer;

		// Spawn server in a thread
		std::thread::spawn(move || {
			serve(addr, server);
		});

		// Give it a moment to start
		std::thread::sleep(Duration::from_millis(100));

		// Connect
		futures_lite::future::block_on(async {
			let mut stream = async_net::TcpStream::connect(addr).await.unwrap();
			stream
				.write_all(b"GET /serve HTTP/1.1\r\n\r\n")
				.await
				.unwrap();

			let mut buf = vec![0u8; 1024];
			let n = stream.read(&mut buf).await.unwrap();
			let response = std::str::from_utf8(&buf[..n]).unwrap();

			assert!(response.contains("Hello /serve"));
		});
	}
}
