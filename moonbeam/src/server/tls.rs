//! TLS config helper module.
use futures_rustls::rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;
use std::fs::File;
use std::io::{BufReader, Error, ErrorKind, Result};
use std::path::Path;

/// Configuration for TLS encryption.
///
/// Create one via [`TlsConfig::from_pem`] to load from files, or
/// [`TlsConfig::from_raw`] to construct programmatically.
pub struct TlsConfig {
	certs: Vec<CertificateDer<'static>>,
	key: PrivateKeyDer<'static>,
}

impl TlsConfig {
	/// Load TLS configuration from PEM-encoded certificate chain and private key files.
	///
	/// `cert_path` should point to a PEM file containing the server certificate, optionally
	/// followed by any intermediate CA certificates.
	///
	/// `key_path` should point to a PEM file containing the private key (PKCS#8 or RSA).
	pub fn from_pem(cert_path: impl AsRef<Path>, key_path: impl AsRef<Path>) -> Result<Self> {
		let cert_file = File::open(cert_path)?;
		let mut cert_reader = BufReader::new(cert_file);
		let certs = rustls_pemfile::certs(&mut cert_reader).collect::<Result<Vec<_>>>()?;

		if certs.is_empty() {
			return Err(Error::new(
				ErrorKind::InvalidInput,
				"No certificates found in the file",
			));
		}

		let key_file = File::open(key_path)?;
		let mut key_reader = BufReader::new(key_file);
		let key = rustls_pemfile::private_key(&mut key_reader)?
			.ok_or_else(|| Error::new(ErrorKind::NotFound, "No private key found in the file"))?;

		Ok(TlsConfig { certs, key })
	}

	/// Construct a `TlsConfig` from raw DER-encoded certificate(s) and private key.
	///
	/// The first certificate in `certs` must be the server certificate; any subsequent
	/// entries are intermediate CA certificates.
	pub fn from_raw(certs: Vec<Vec<u8>>, key: Vec<u8>) -> Self {
		TlsConfig {
			certs: certs.into_iter().map(CertificateDer::from).collect(),
			key: PrivateKeyDer::Pkcs8(key.into()),
		}
	}

	/// Convert this configuration into an [`ServerConfig`] for use with
	/// [`futures_rustls::TlsAcceptor`].
	pub fn into_server_config(self) -> Result<ServerConfig> {
		let mut config = ServerConfig::builder()
			.with_no_client_auth()
			.with_single_cert(self.certs, self.key)
			.map_err(|e| Error::new(ErrorKind::InvalidInput, e))?;

		config.alpn_protocols = vec![b"http/1.1".to_vec()];

		Ok(config)
	}
}
