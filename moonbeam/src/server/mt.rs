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
#[cfg(feature = "tls")]
use rustls::ServerConfig;
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
///     async fn route<'server: 'exec, 'exec>(
///         &'server self,
///         _req: Request<'_, '_>,
///         _spawner: Spawner<'exec>,
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
pub fn serve_multi<F, T: Server>(
	addr: impl AsyncToSocketAddrs,
	num_threads: ThreadCount,
	factory: F,
) where
	F: FnOnce() -> T + Send + Clone,
{
	let _span = tracing::trace_span!("thread", id = "main").entered();
	let num_threads = resolve_thread_count(num_threads);
	tracing::debug!(num_threads, "Starting worker threads");

	thread::scope(|scope| {
		let (send, recv) = flume::bounded::<(TcpStream, SocketAddr)>(num_threads * 100);
		let (main_done_shutdown, worker_force_shutdown, worker_done_shutdown, all_workers_shutdown) =
			make_signals();

		for _i in 0..num_threads {
			let server = factory.clone();
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

fn resolve_thread_count(num_threads: ThreadCount) -> usize {
	match num_threads {
		ThreadCount::Default => thread::available_parallelism()
			.map(NonZeroUsize::get)
			.unwrap_or(1),
		ThreadCount::Count(n) => n,
	}
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

/// Serves a multi-threaded HTTPS server using TLS.
///
/// This clones the TLS configuration across multiple worker threads to achieve
/// a share-nothing architecture without locks.
#[cfg(feature = "tls")]
#[cfg_attr(docsrs, doc(cfg(all(feature = "mt", feature = "tls"))))]
pub fn serve_multi_tls<F, T: Server>(
	addr: impl AsyncToSocketAddrs,
	num_threads: ThreadCount,
	tls_config: ServerConfig,
	factory: F,
) where
	F: FnOnce() -> T + Send + Clone,
{
	use futures_rustls::TlsAcceptor;
	use std::sync::Arc;

	let _span = tracing::trace_span!("thread", id = "main").entered();
	let num_threads = resolve_thread_count(num_threads);
	tracing::debug!(num_threads, "Starting worker threads");

	thread::scope(|scope| {
		let (send, recv) = flume::bounded::<(TcpStream, SocketAddr)>(num_threads * 100);
		let (main_done_shutdown, worker_force_shutdown, worker_done_shutdown, all_workers_shutdown) =
			make_signals();

		let acceptor = TlsAcceptor::from(Arc::new(tls_config));
		for _i in 0..num_threads {
			let server = factory.clone();
			let worker_done_shutdown = worker_done_shutdown.clone();
			let worker_force_shutdown = worker_force_shutdown.clone();
			let recv = recv.clone();
			let acceptor = acceptor.clone();
			scope.spawn(move || {
				let server = server();
				let executor = Executor::new();
				let spawner = executor.spawner();

				let _span = tracing::trace_span!("thread", id = _i).entered();
				async_io::block_on(executor.run(async {
					while let Ok((socket, addr)) = recv.recv_async().await {
						let _ = socket.set_nodelay(true);
						let acceptor = acceptor.clone();
						let server_ref = &server;
						spawner.spawn(async move {
							match acceptor.accept(socket).await {
								Ok(tls_stream) => {
									handle_socket(tls_stream, addr, server_ref, spawner).await;
								}
								Err(_err) => {
									tracing::error!(?_err, "TLS handshake failed");
								}
							}
						});
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

#[cfg(all(test, feature = "tls"))]
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
	fn test_serve_multi_tls() {
		use async_net::TcpListener;
		use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName};
		use std::sync::Arc;
		use std::time::Duration;

		// Generate cert
		let subject_alt_names = vec!["127.0.0.1".to_string(), "localhost".to_string()];
		let cert = rcgen::generate_simple_self_signed(subject_alt_names).unwrap();
		let cert_der = cert.cert.der().to_vec();
		let key_der = cert.signing_key.serialize_der();

		let certs = vec![CertificateDer::from(cert_der.clone())];
		let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der));
		let server_config = rustls::ServerConfig::builder()
			.with_no_client_auth()
			.with_single_cert(certs, key)
			.unwrap();

		// Pick a random port
		let addr = futures_lite::future::block_on(async {
			let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
			listener.local_addr().unwrap()
		});

		// Spawn server in a thread
		let server_config_clone = server_config.clone();
		std::thread::spawn(move || {
			crate::server::mt::serve_multi_tls(
				addr,
				crate::ThreadCount::Count(2),
				server_config_clone,
				|| MockServer,
			);
		});

		// Give it a moment to start
		std::thread::sleep(Duration::from_millis(150));

		// Connect as client
		futures_lite::future::block_on(async {
			let mut root_store = rustls::RootCertStore::empty();
			root_store.add(CertificateDer::from(cert_der)).unwrap();
			let client_config = rustls::ClientConfig::builder()
				.with_root_certificates(root_store)
				.with_no_client_auth();

			let connector = futures_rustls::TlsConnector::from(Arc::new(client_config));
			let stream = async_net::TcpStream::connect(addr).await.unwrap();
			let domain = ServerName::try_from("127.0.0.1").unwrap();
			let mut tls_stream = connector.connect(domain, stream).await.unwrap();

			tls_stream
				.write_all(b"GET /serve_multi_tls HTTP/1.1\r\n\r\n")
				.await
				.unwrap();

			let mut buf = vec![0u8; 1024];
			let n = tls_stream.read(&mut buf).await.unwrap();
			let response = std::str::from_utf8(&buf[..n]).unwrap();

			assert!(response.contains("Hello /serve_multi_tls"));
		});
	}
}
