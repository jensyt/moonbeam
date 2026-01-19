# Moonbeam Project Context for Agents

## Project Overview
**Moonbeam** is a single-threaded-first, asynchronous HTTP server written in Rust. It is designed to be simple and efficient by running on a single thread by default, eliminating the need for thread synchronization primitives like `Arc` and `Mutex`. However, it also supports a "share-nothing" multi-threaded mode via the `mt` feature.

## Core Philosophy
1.  **Single-Threaded by Default**: The server runs on a single thread to avoid context switching and synchronization overhead.
2.  **Async I/O**: It uses `async-io` and `futures-lite` for non-blocking operations.
3.  **No Synchronization Needed**: State management should rely on `std::cell::Cell` or `std::cell::RefCell` for interior mutability, rather than thread-safe alternatives. Multi-threading is achieved by replicating state across threads.

## Workspace Structure
The project is a Cargo workspace with the following members:
- **`moonbeam/`**: The core server library. Contains HTTP handling, server logic, routing, and utilities.
- **`moonbeam-attributes/`**: Procedural macros, including `#[server]`, `#[route]`, and `router!`.

## Key Components
- **`#[server]` Macro**: Used to define simple request handlers.
- **Routing System**: `#[route]`, `#[middleware]`, and `router!` macros for defining complex routing trees with middleware support.
- **`Request` & `Response`**: Core types for HTTP exchange.
- **`assets`**: Utilities for serving static files with ETag support. Now uses blocking I/O for robustness.
- **`serve_multi`**: Function to run the server in multi-threaded mode (requires `mt` feature).

## Development Guidelines
- **Concurrency**: Do NOT introduce `std::sync::Arc`, `std::sync::Mutex`, or `tokio::spawn` unless absolutely necessary. For multi-threading, rely on the "share-nothing" architecture where state is cloned per thread.
- **Async Runtime**: The project uses `async-executor` (via `async-io` ecosystem), not Tokio.
- **Error Handling**: Panics in handlers are caught by the server (if `catchpanic` feature is enabled), but prefer returning proper error responses.

## Development Workflow
- **Formatting**: Always format code using `cargo fmt`.
- **Testing**: Run tests with `cargo test`. To ensure all features are covered, use `cargo test --all-features`.
- **Documentation**: Generate and view documentation with `cargo doc --open`.
- **Linting**: Use `cargo clippy` to check for idiomatic Rust code.
