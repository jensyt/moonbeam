#![doc = include_str!("../README.md")]
#![cfg_attr(docsrs, feature(doc_cfg))]

#[cfg(feature = "assets")]
#[cfg_attr(docsrs, doc(cfg(feature = "assets")))]
pub mod assets;
pub mod http;
#[cfg(feature = "router")]
#[cfg_attr(docsrs, doc(cfg(feature = "router")))]
pub mod router;
pub mod server;
#[macro_use]
mod tracing;

pub use crate::http::{Body, Request, Response};
#[cfg(all(feature = "tls", feature = "mt"))]
#[cfg_attr(docsrs, doc(cfg(all(feature = "tls", feature = "mt"))))]
pub use crate::server::mt::serve_multi_tls;
#[cfg(feature = "mt")]
#[cfg_attr(docsrs, doc(cfg(feature = "mt")))]
pub use crate::server::mt::{ThreadCount, serve_multi};
pub use crate::server::task::{Executor, Spawner};
pub use crate::server::{Server, st::serve};
#[cfg(feature = "tls")]
#[cfg_attr(docsrs, doc(cfg(feature = "tls")))]
pub use crate::server::{st::serve_tls, tls::TlsConfig};
pub use httparse::Header;

/// Attribute macro to simplify creating server implementations.
#[cfg(feature = "macros")]
#[cfg_attr(docsrs, doc(cfg(feature = "macros")))]
pub use moonbeam_attributes::server;
#[cfg(feature = "router")]
#[cfg_attr(docsrs, doc(cfg(feature = "router")))]
pub use moonbeam_attributes::{middleware, route, router};
