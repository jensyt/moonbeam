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
use crate::tracing::{self, Instrument};
use async_net::{AsyncToSocketAddrs, TcpListener};
#[cfg(feature = "tls")]
use rustls::ServerConfig;
use std::io::ErrorKind;
use std::pin::pin;

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
///     async fn route<'server: 'exec, 'exec>(
///         &'server self,
///         _req: Request<'_, '_>,
///         _spawner: Spawner<'exec>,
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
	let executor = pin!(Executor::new());
	let spawner = executor.as_ref().spawner();
	async_io::block_on(executor.run(async {
		let listener = TcpListener::bind(addr)
			.await
			.expect("Failed to bind to socket");
		accept_loop(listener, &server, spawner).await;
	}));
}

async fn accept_loop<'server: 'exec, 'exec, S: Server>(
	listener: TcpListener,
	server: &'server S,
	spawner: Spawner<'exec>,
) {
	let mut gate = SignalGate::new();

	loop {
		match gate.or_signal(listener.accept()).await {
			Ok((socket, _addr)) => {
				let _ = socket.set_nodelay(true);
				spawner.spawn(
					handle_socket(socket, server, spawner)
						.instrument(tracing::info_span!("conn", remote = %_addr)),
				);
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

	gate.wait_for_shutdown(spawner).await;
}

/// Serves a single-threaded HTTPS server using TLS.
///
/// Under the hood, TLS handshakes are processed asynchronously in spawned tasks
/// to ensure that the accept loop is not blocked by slow TLS handshakes.
#[cfg(feature = "tls")]
#[cfg_attr(docsrs, doc(cfg(feature = "tls")))]
pub fn serve_tls<F, T>(addr: impl AsyncToSocketAddrs, tls_config: ServerConfig, factory: F)
where
	F: FnOnce() -> T,
	T: Server,
{
	let server = factory();
	let executor = pin!(Executor::new());
	let spawner = executor.as_ref().spawner();
	async_io::block_on(executor.run(async {
		let listener = TcpListener::bind(addr)
			.await
			.expect("Failed to bind to socket");
		accept_loop_tls(listener, tls_config, &server, spawner).await;
	}));
}

#[cfg(feature = "tls")]
async fn accept_loop_tls<'server: 'exec, 'exec, S: Server>(
	listener: TcpListener,
	tls_config: ServerConfig,
	server: &'server S,
	spawner: Spawner<'exec>,
) {
	use futures_rustls::TlsAcceptor;
	use std::sync::Arc;

	let mut gate = SignalGate::new();
	let acceptor = TlsAcceptor::from(Arc::new(tls_config));

	loop {
		match gate.or_signal(listener.accept()).await {
			Ok((socket, _addr)) => {
				let _ = socket.set_nodelay(true);
				let acceptor = acceptor.clone();
				spawner.spawn(async move {
					match acceptor.accept(socket).await {
						Ok(tls_stream) => {
							handle_socket(tls_stream, server, spawner)
								.instrument(tracing::info_span!("conn", remote = %_addr))
								.await;
						}
						Err(_err) => {
							tracing::debug!(error = ?_err, "TLS handshake failed");
						}
					}
				});
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

	#[test]
	#[cfg(feature = "tls")]
	fn test_serve_tls() {
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
			serve_tls(addr, server_config_clone, || MockServer);
		});

		// Give it a moment to start
		std::thread::sleep(Duration::from_millis(100));

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
				.write_all(b"GET /serve_tls HTTP/1.1\r\n\r\n")
				.await
				.unwrap();

			let mut buf = vec![0u8; 1024];
			let n = tls_stream.read(&mut buf).await.unwrap();
			let response = std::str::from_utf8(&buf[..n]).unwrap();

			assert!(response.contains("Hello /serve_tls"));
		});
	}
}
