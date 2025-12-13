//! # Moonbeam
//!
//! Moonbeam is a lightweight, single-threaded HTTP server library for Rust.
//! It is designed for simplicity and performance, avoiding `Rc` and `Arc` where possible.
//!
//! Key features:
//! - Single-threaded execution model
//! - Async/await support
//! - Built-in HTTP parsing and routing
//! - Minimal dependencies

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
