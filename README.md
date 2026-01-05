# Moonbeam

A single-threaded async HTTP server written in Rust.

Moonbeam is designed to be simple, efficient, and free of synchronization overhead by running on a single thread. It leverages `async-io` and `futures-lite` to handle concurrent connections asynchronously.

## Features

- **Single-threaded Architecture**: No `Arc` or `Mutex` needed for shared state.
- **Async I/O**: Efficiently handles many connections using non-blocking I/O.
- **Simple API**: Use the `#[server]` macro to turn functions into server handlers.
- **Advanced Routing**: `router!` macro supports nested groups, middleware chaining, fallbacks, path parameters, and wildcards.
- **Static Assets**: Built-in support for serving files with ETags and MIME type detection.
- **HTTP/1.1**: Supports persistent connections, chunked transfer encoding, and common headers.
- **Standard Features**: Includes support for Cookies, Query Parameters, Headers, and Bodies.
- **Panic Handling**: Robust server that catches panics in request handlers.
- **Response Compression**: Supports automatic compression of responses (Gzip, Brotli, Zlib).
- **Graceful Shutdown**: Handles signals for clean shutdown.

## Installation

Add `moonbeam` to your `Cargo.toml`:

```toml
[dependencies]
moonbeam = "0.2.2"
```

## Feature Flags

Moonbeam provides several feature flags to configure functionality and dependencies:

- **default**: Enables `macros`, `assets`, `asyncfs`, `catchpanic`, `signals`, and `router`.
- **macros**: Enables the `#[server]` attribute macro.
- **assets**: Enables static file serving utilities (implies `asyncfs`).
- **asyncfs**: Enables asynchronous file system support.
- **signals**: Enables signal handling (e.g., for graceful shutdown).
- **catchpanic**: Wraps handlers to catch panics and return 500 errors.
- **tracing**: Enables `tracing` instrumentation.
- **compress**: Enables HTTP response compression (gzip, brotli, zlib).
- **router**: Enables the routing system (`#[route]` and `router!` macros).

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

## Routing

Moonbeam offers a flexible routing system via the `router` feature (enabled by default). It supports nested routes, middleware, and path extractors.

```rust
use moonbeam::{Response, route, router, serve, middleware};
use moonbeam::router::PathParams;

struct AppState {
    api_key: String,
}

// Define middleware using the #[middleware] attribute
#[middleware]
async fn logger(req: Request, _state: &AppState, next: Next) -> Response {
    println!("Log: {} {}", req.method, req.url());
    next(req).await
}

#[middleware]
async fn auth(req: Request, state: &AppState, next: Next) -> Response {
    if let Some(key) = req.find_header("X-Api-Key") {
        if key == state.api_key.as_bytes() {
            return next(req).await;
        }
    }
    Response::new_with_code(401).with_body("Unauthorized", Some("text/plain"))
}

#[route]
async fn hello(PathParams(name): PathParams<&str>) -> Response {
    Response::new_with_body(format!("Hello, {}!", name), Some("text/plain"))
}

#[route]
async fn not_found() -> Response {
    Response::new_with_code(404).with_body("Custom 404", Some("text/plain"))
}

fn main() {
    // Define the router
    router!(MyRouter<AppState> {
        // Global middleware
        with logger

        get("/hello/:name") => hello,

        // Route group with prefix
        "/api" => {
            // Middleware for this group
            with auth

            get("/status") => hello, // Reusing handler for demo
            
            // Nested group
            "/v1" => {
                post("/save") => hello
            }
            
            // Fallback for /api/* (matches if no other route in /api matches)
            _ => ! // ! means standard 404
        }
        
        // Global fallback
        _ => not_found
    });

    let state = AppState { api_key: "secret".to_string() };
    let app = MyRouter::new(state);
    
    serve("127.0.0.1:8080", app);
}
```

## License

This project is licensed under the MIT License.
