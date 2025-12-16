# Moonbeam

A single-threaded async HTTP server written in Rust.

Moonbeam is designed to be simple, efficient, and free of synchronization overhead by running on a single thread. It leverages `async-io` and `futures-lite` to handle concurrent connections asynchronously.

## Features

- **Single-threaded Architecture**: No `Arc` or `Mutex` needed for shared state.
- **Async I/O**: Efficiently handles many connections using non-blocking I/O.
- **Simple API**: Use the `#[server]` macro to turn functions into server handlers.
- **Static Assets**: Built-in support for serving files with ETags and MIME type detection.
- **HTTP/1.1**: Supports persistent connections, chunked transfer encoding, and common headers.
- **Standard Features**: Includes support for Cookies, Query Parameters, Headers, and Bodies.
- **Panic Handling**: Robust server that catches panics in request handlers.

## Installation

Add `moonbeam` to your `Cargo.toml`:

```toml
[dependencies]
moonbeam = "0.2"
```

## Usage

### Basic Example

```rust
use moonbeam::{Request, Response, server};

#[server(HelloWorld)]
async fn serve(_request: Request<'_, '_>) -> Response {
    Response::new_with_body("Hello, World!", Some("text/plain"))
}

fn main() {
    println!("Running on 127.0.0.1:8080");
    moonbeam::serve("127.0.0.1:8080", HelloWorld);
}
```

### State Management

Since Moonbeam is single-threaded, you can use `Cell` or `RefCell` for interior mutability without thread-safe primitives.

```rust
use std::cell::Cell;
use moonbeam::{Request, Response, server};

struct State {
    count: Cell<u64>,
}

#[server(CounterServer)]
async fn serve(_req: Request<'_, '_>, state: &'static State) -> Response {
    let count = state.count.get();
    state.count.set(count + 1);
    Response::new_with_body(format!("Request #{}", count), None)
}

fn main() {
    let state = State { count: Cell::new(0) };
    moonbeam::serve("127.0.0.1:8080", CounterServer(state));
}
```

### Serving Static Files

Moonbeam includes a helper for serving static assets.

```rust
use moonbeam::{Request, Response, server, assets::get_asset};

#[server(FileServer)]
async fn serve(req: Request<'_, '_>) -> Response {
    // Serve files from the current directory
    let etag = req.find_header("If-None-Match");
    get_asset(req.path, etag, ".")
}
```

## License

This project is licensed under the MIT License.
