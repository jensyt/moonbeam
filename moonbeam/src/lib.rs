#[cfg(feature = "assets")]
pub mod assets;
pub mod http;
pub mod server;
#[macro_use]
mod tracing;

pub use crate::http::{Body, Request, Response};
pub use crate::server::task::new_local_task;
pub use crate::server::{Server, serve};
pub use httparse::Header;

#[cfg(feature = "macros")]
pub use moonbeam_attributes::server;
