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
//! # Examples
//!
//! ## Stateless Server
//!
//! ```no_run
//! use moonbeam::{Request, Response, server, serve};
//!
//! #[server(MyServer)]
//! async fn handle_request(_req: Request) -> Response {
//!     Response::ok().with_body("Hello, World!", None)
//! }
//!
//! fn main() {
//!     serve("127.0.0.1:8080", MyServer);
//! }
//! ```
//!
//! ## Stateful Server
//!
//! ```no_run
//! use moonbeam::{Request, Response, server, serve};
//!
//! struct State {
//!     count: std::sync::atomic::AtomicUsize,
//! }
//!
//! #[server(MyStatefulServer)]
//! async fn handle_request(_req: Request, state: &State) -> Response {
//!     let count = state.count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
//!     Response::ok().with_body(format!("Request count: {}", count), None)
//! }
//!
//! fn main() {
//!     let state = State {
//!         count: std::sync::atomic::AtomicUsize::new(0),
//!     };
//!     // Pass the state to the generated struct tuple constructor
//!     serve("127.0.0.1:8080", MyStatefulServer(state));
//! }
//! ```

#[cfg(feature = "assets")]
pub mod assets;
pub mod http;
#[cfg(feature = "router")]
pub mod router;
pub mod server;
#[macro_use]
mod tracing;

pub use crate::http::{Body, Request, Response};
pub use crate::server::task::new_local_task;
pub use crate::server::{Server, serve};
pub use httparse::Header;

/// Attribute macro to simplify creating server implementations.
#[cfg(feature = "macros")]
pub use moonbeam_attributes::server;
#[cfg(feature = "router")]
pub use moonbeam_attributes::{middleware, route, router};
