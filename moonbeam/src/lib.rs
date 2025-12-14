//! # Moonbeam
//!
//! Moonbeam is a lightweight, single-threaded HTTP server library for Rust.
//! It is designed for simplicity and performance.
//!
//! Key features:
//! - Single-threaded execution model
//! - Async/await support
//! - Built-in HTTP parsing and routing
//! - Minimal dependencies
//!
//! # Example
//!
//! ```no_run
//! use moonbeam::{Server, Request, Response, serve};
//! use std::future::Future;
//!
//! struct MyServer;
//!
//! impl Server for MyServer {
//!     fn route(&'static self, _req: Request) -> impl Future<Output = Response> {
//!         async {
//!             Response::ok().with_body("Hello, World!", None)
//!         }
//!     }
//! }
//!
//! fn main() {
//!     serve("127.0.0.1:8080", MyServer);
//! }
//! ```

pub mod http;
pub mod server;
#[cfg(feature = "assets")]
pub mod assets;
#[macro_use]
mod tracing;

pub use crate::server::{Server, serve};
pub use crate::server::task::new_local_task;
pub use crate::http::{Request, Response, Body};
pub use httparse::Header;

/// Attribute macro to simplify creating server implementations.
#[cfg(feature = "macros")]
pub use moonbeam_attributes::server;
