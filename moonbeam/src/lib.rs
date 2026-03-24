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
#[cfg(feature = "mt")]
#[cfg_attr(docsrs, doc(cfg(feature = "mt")))]
pub use crate::server::mt::{ThreadCount, serve_multi};
pub use crate::server::task::new_local_task;
pub use crate::server::{Server, st::serve};
pub use httparse::Header;

/// Attribute macro to simplify creating server implementations.
#[cfg(feature = "macros")]
#[cfg_attr(docsrs, doc(cfg(feature = "macros")))]
pub use moonbeam_attributes::server;
#[cfg(feature = "router")]
#[cfg_attr(docsrs, doc(cfg(feature = "router")))]
pub use moonbeam_attributes::{middleware, route, router};
