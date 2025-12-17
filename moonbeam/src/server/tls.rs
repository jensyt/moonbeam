#[cfg(feature = "tls")]
use futures_lite::{AsyncRead, AsyncWrite};
#[cfg(feature = "tls")]
pub use futures_rustls::TlsAcceptor;
#[cfg(feature = "tls")]
use futures_rustls::rustls::{
	ServerConfig,
	pki_types::{CertificateDer, PrivateKeyDer},
};
#[cfg(feature = "tls")]
use std::{fs::File, io::BufReader, path::Path, sync::Arc};

#[cfg(feature = "tls")]
pub fn load_certs(path: &Path) -> std::io::Result<Vec<CertificateDer<'static>>> {
	let file = File::open(path)?;
	let mut reader = BufReader::new(file);
	rustls_pemfile::certs(&mut reader).collect()
}

#[cfg(feature = "tls")]
pub fn load_private_key(path: &Path) -> std::io::Result<PrivateKeyDer<'static>> {
	let file = File::open(path)?;
	let mut reader = BufReader::new(file);
	rustls_pemfile::private_key(&mut reader)?
		.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "no private key found"))
}

#[cfg(feature = "tls")]
pub fn create_tls_acceptor(cert_path: &Path, key_path: &Path) -> std::io::Result<TlsAcceptor> {
	let certs = load_certs(cert_path)?;
	let key = load_private_key(key_path)?;

	let config = ServerConfig::builder()
		.with_no_client_auth()
		.with_single_cert(certs, key)
		.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

	Ok(TlsAcceptor::from(Arc::new(config)))
}

/// A wrapper to make futures::io::AsyncRead/Write compatible with futures_lite::io::AsyncRead/Write
/// This wraps the TlsStream (which implements futures::io) to expose futures_lite::io traits.
#[cfg(feature = "tls")]
pub struct TlsStreamCompat<S>(pub futures_rustls::server::TlsStream<S>);

#[cfg(feature = "tls")]
impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for TlsStreamCompat<S> {
	fn poll_read(
		mut self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
		buf: &mut [u8],
	) -> std::task::Poll<std::io::Result<usize>> {
		futures_io::AsyncRead::poll_read(std::pin::Pin::new(&mut self.0), cx, buf)
	}
}

#[cfg(feature = "tls")]
impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for TlsStreamCompat<S> {
	fn poll_write(
		mut self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
		buf: &[u8],
	) -> std::task::Poll<std::io::Result<usize>> {
		futures_io::AsyncWrite::poll_write(std::pin::Pin::new(&mut self.0), cx, buf)
	}

	fn poll_flush(
		mut self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
	) -> std::task::Poll<std::io::Result<()>> {
		futures_io::AsyncWrite::poll_flush(std::pin::Pin::new(&mut self.0), cx)
	}

	fn poll_close(
		mut self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
	) -> std::task::Poll<std::io::Result<()>> {
		futures_io::AsyncWrite::poll_close(std::pin::Pin::new(&mut self.0), cx)
	}
}

/// A wrapper to make futures_lite::io::AsyncRead/Write compatible with futures::io::AsyncRead/Write.
/// This wraps the socket (which implements futures_lite::io) to expose futures::io traits required by TlsAcceptor.
#[cfg(feature = "tls")]
pub struct FuturesIoCompat<S>(pub S);

#[cfg(feature = "tls")]
impl<S: AsyncRead + Unpin> futures_io::AsyncRead for FuturesIoCompat<S> {
	fn poll_read(
		mut self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
		buf: &mut [u8],
	) -> std::task::Poll<std::io::Result<usize>> {
		futures_lite::AsyncRead::poll_read(std::pin::Pin::new(&mut self.0), cx, buf)
	}
}

#[cfg(feature = "tls")]
impl<S: AsyncWrite + Unpin> futures_io::AsyncWrite for FuturesIoCompat<S> {
	fn poll_write(
		mut self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
		buf: &[u8],
	) -> std::task::Poll<std::io::Result<usize>> {
		futures_lite::AsyncWrite::poll_write(std::pin::Pin::new(&mut self.0), cx, buf)
	}

	fn poll_flush(
		mut self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
	) -> std::task::Poll<std::io::Result<()>> {
		futures_lite::AsyncWrite::poll_flush(std::pin::Pin::new(&mut self.0), cx)
	}

	fn poll_close(
		mut self: std::pin::Pin<&mut Self>,
		cx: &mut std::task::Context<'_>,
	) -> std::task::Poll<std::io::Result<()>> {
		futures_lite::AsyncWrite::poll_close(std::pin::Pin::new(&mut self.0), cx)
	}
}
