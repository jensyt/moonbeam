# Moonbeam

A single-threaded-first async HTTP/1.1 server written in Rust.

Moonbeam is designed to be simple, efficient, and free of synchronization overhead by running on a single thread, with the ability to extend to multiple threads using a "share nothing" philosophy if desired. It leverages `async-io` and `futures-lite` to handle concurrent connections asynchronously.

## Features

- **Single-threaded by default**: No `Arc` or `Mutex` needed for shared state.
- **Multi-threaded support**: Each thread gets a copy of state by default, but you can add synchronization of shared state if needed.
- **Async I/O**: Efficiently handles many connections using non-blocking I/O.
- **Simple API**: Use the `#[server]` macro to turn functions into server handlers.
- **Routing**: `router!` macro supports nested groups, middleware chaining, fallbacks, path parameters, and wildcards.
- **Static Assets**: Built-in support for serving files with ETags and MIME type detection.
- **HTTP/1.1**: Supports persistent connections, chunked transfer encoding, and common headers.
- **Standard Features**: Includes support for Cookies, Query Parameters, Headers, and Bodies.
- **Panic Handling**: Can catch panics in request handlers if desired.
- **Response Compression**: Supports automatic compression of responses (Gzip, Brotli, Zlib).
- **Graceful Shutdown**: Optionally handles signals for clean shutdown.

## Installation

Add `moonbeam` to your `Cargo.toml`:

```toml
[dependencies]
moonbeam = "0.3.0"
```

## Feature Flags

Moonbeam provides several feature flags to configure functionality and dependencies:

- **default**: Enables `macros`, `assets`, `catchpanic`, `signals`, and `router`.
- **macros**: Enables the `#[server]` attribute macro.
- **assets**: Enables static file serving utilities.
- **signals**: Enables signal handling (e.g., for graceful shutdown).
- **catchpanic**: Wraps handlers to catch panics and return 500 errors.
- **tracing**: Enables `tracing` instrumentation.
- **compress**: Enables HTTP response compression (gzip, brotli, zlib).
- **router**: Enables the routing system (`#[route]` and `router!` macros).
- **mt**: Enables multi-threading support (`serve_multi`).

## Usage

### Basic Example

```rust
use moonbeam::{Request, Response, server};

#[server(HelloWorld)]
async fn serve(_request: Request) -> Response {
    Response::new_with_body("Hello, World!", Some("text/plain"))
}

fn main() {
    println!("Running on 127.0.0.1:8080");
    moonbeam::serve("127.0.0.1:8080", HelloWorld);
}
```

### State Management

Since Moonbeam is single-threaded by default, you can use `Cell` or `RefCell` for interior mutability without thread-safe primitives.

```rust
use std::cell::Cell;
use moonbeam::{Request, Response, server};

struct State {
    count: Cell<u64>,
}

#[server(CounterServer)]
async fn serve(_req: Request, state: &'static State) -> Response {
    let count = state.count.get();
    state.count.set(count + 1);
    Response::new_with_body(format!("Request #{}", count), None)
}

fn main() {
    let state = State { count: Cell::new(0) };
    moonbeam::serve("127.0.0.1:8080", CounterServer(state));
}
```

### Multi-threaded Server

To utilize multiple cores, use `serve_multi` (requires `mt` feature). Moonbeam uses a "share-nothing" approach by default where the server state is replicated for each thread. To share between threads, you can optionally use `Arc` or atomics.

```rust
use moonbeam::{Request, Response, ThreadCount, server, serve_multi};
use std::sync::atomic::{AtomicUsize, Ordering};

struct State {
    thread_id: usize,
}

#[server(Worker)]
async fn serve(_req: Request, state: &State) -> Response {
    Response::new_with_body(format!("Hello from thread {}", state.thread_id), None)
}

fn main() {
    let next_id = AtomicUsize::new(0);

    serve_multi(
        "127.0.0.1:8080",
        ThreadCount::Default, // Uses available parallelism
        || {
            let id = next_id.fetch_add(1, Ordering::Relaxed);
            Worker(State { thread_id: id })
        },
        |_| {} // No cleanup needed
    );
}
```

### Serving Static Files

Moonbeam includes a helper for serving static assets.

```rust
use moonbeam::{Request, Response, server, assets::get_asset};

#[server(FileServer)]
async fn serve(req: Request) -> Response {
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
