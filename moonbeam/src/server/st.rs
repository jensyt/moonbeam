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

use super::signal_gate::SignalGate;
use super::task::{Executor, Spawner};
use super::{Server, handle_socket};
use crate::tracing;
use async_net::{AsyncToSocketAddrs, TcpListener};
use std::io::ErrorKind;

/// Starts the server on the specified address using a single-threaded local executor.
///
/// This function blocks the current thread and runs the server loop, driving all incoming async
/// connections concurrently on a single thread. Because it uses a `LocalExecutor`, you can safely
/// use non-Send/Sync types like `Cell` and `RefCell` in your server state.
///
/// For consistency with [`serve_multi`], this function takes a state factory that is called once
/// to create the server state.
///
/// # Warning
/// CPU-heavy operations or synchronous blocking I/O inside your handlers will block the entire
/// server. Use `blocking::unblock` for these tasks.
///
/// # Example
/// ```no_run
/// use moonbeam::{Server, Request, Response, Spawner, serve};
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
/// serve("127.0.0.1:8080", || MyServer);
/// ```
pub fn serve<F, T>(addr: impl AsyncToSocketAddrs, factory: F)
where
	F: FnOnce() -> T,
	T: Server,
{
	let server = factory();
	let executor = Executor::new();
	let spawner = executor.spawner();
	async_io::block_on(executor.run(async {
		let listener = TcpListener::bind(addr)
			.await
			.expect("Failed to bind to socket");
		accept_loop(listener, &server, spawner).await;
	}));
}

async fn accept_loop<'s: 'e, 'e, T: Server>(
	listener: TcpListener,
	server: &'s T,
	spawner: Spawner<'e>,
) {
	let mut gate = SignalGate::new();

	loop {
		match gate.or_signal(listener.accept()).await {
			Ok((socket, addr)) => {
				let _ = socket.set_nodelay(true);
				spawner.spawn(handle_socket(socket, addr, server, spawner));
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

	gate.wait_for_shutdown(spawner).await;
}

#[cfg(test)]
mod test {
	use super::*;
	use crate::http::{Body, Request, Response};
	use futures_lite::{AsyncReadExt, AsyncWriteExt};

	struct MockServer;
	impl Server for MockServer {
		async fn route<'s: 'e, 'e>(
			&'s self,
			req: Request<'_, '_>,
			_spawner: Spawner<'e>,
		) -> Response {
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

		// Spawn server in a thread
		std::thread::spawn(move || {
			serve(addr, || MockServer);
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
