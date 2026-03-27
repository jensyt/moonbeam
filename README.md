# Moonbeam

A single-threaded-first async HTTP/1.1 server written in Rust.

Moonbeam is designed to be simple, efficient, and free of synchronization overhead by running on a single thread. It leverages the `async-io` and `smol` ecosystem to handle concurrent connections asynchronously. By default, it uses a "share-nothing" architecture, avoiding the need for `Arc`, `Mutex`, or `Send`/`Sync` bounds on your state, though it can easily be extended to multiple threads if desired.

## Motivation
Modern web applications often spend most of their time waiting on I/O (databases, network requests, etc.) rather than performing heavy CPU computation. Moonbeam embraces this by running your application logic on a single thread, utilizing a local executor. This means you can use simple `RefCell` and `Cell` primitives for state management, drastically reducing the cognitive overhead and boilerplate often associated with multi-threaded Rust web frameworks. 

## Critical Considerations

Before building with Moonbeam, it's essential to understand its execution model:

- **No Tokio**: Moonbeam is built on `async-io` and the `smol` ecosystem. **It does not use `tokio` dependencies**. This means no `tokio::spawn`, no `#[tokio::main]`, and no tokio-specific database drivers (unless they support `async-io` or `smol`).
- **Blocking I/O**: Because Moonbeam runs handlers on a `LocalExecutor` on the main thread, any CPU-heavy computation or blocking I/O (like reading a large file synchronously) **will block the entire server**.
  - *Solution*: `smol` supports async I/O via the `blocking::unblock` primitive for offloading heavy tasks to a background thread pool, or you can use the `async_io` crate for native non-blocking operations.
- **Static Lifetimes & State**: To satisfy the executor's requirements, the server instance and its state are typically leaked to the `'static` lifetime using `Box::leak` (which is what `moonbeam::serve` does internally). This is a safe and common pattern for long-lived server processes, and Moonbeam offers a function (`moonbeam::Server::destroy`) to handle dropping the state object when it is no longer needed.

## Features

- **Single-threaded by default**: No `Arc` or `Mutex` needed for shared state.
- **Multi-threaded support**: The `mt` feature spawns worker threads, each with its own state copy.
- **Simple API**: Use the `#[server]` macro to turn functions into server handlers.
- **Routing**: The `router!` macro provides a clean DSL and efficient implementation for nested groups, middleware, path parameters, and wildcards.
- **Static Assets**: Built-in `assets` helper for serving files with ETags and MIME type detection.
- **HTTP/1.1**: Persistent connections, chunked transfer encoding, and standard header parsing.
- **Zero-cost extractions**: Efficient parsing of Cookies, Query Parameters, and Bodies.
- **Panic Handling**: Optional `catchpanic` feature safely catches panics and returns a 500 error.
- **Response Compression**: On-the-fly `compress` support (Gzip, Brotli, Zlib).
- **Graceful Shutdown**: Intercepts `signals` for clean exit.

## Is it fast?

Yes. Moonbeam is designed for high performance with minimal overhead. In simple benchmarks using `wrk` (4 threads, 100 connections, 5 seconds), Moonbeam shows competitive performance for both simple responses and static file serving.

*The below benchmarks were performed on a MacBook Pro (M3 Pro). While these simple tests don't represent real-world application complexity, they demonstrate the efficiency of Moonbeam's core request/response loop.*

### Hello World (Plain Text)

| Framework | Architecture | Requests/sec |
| :--- | :--- | :--- |
| **Moonbeam** | **Multi-Threaded (4 cores)** | **~214,000** |
| **Moonbeam** | **Single-Threaded** | **~211,000** |
| Node.js | Single-Threaded | ~117,000 |
| Rouille | Thread-per-connection | ~93,000 |

### Static File Serving (4KB file)

| Framework | Architecture | Requests/sec |
| :--- | :--- | :--- |
| **Moonbeam** | **Multi-Threaded (4 cores)** | **~73,000** |
| **Moonbeam** | **Single-Threaded** | **~66,000** |
| Rouille | Thread-per-connection | ~56,000 |
| Node.js | Single-Threaded | ~51,000 |

## Installation

Add `moonbeam` to your `Cargo.toml`:

```toml
[dependencies]
moonbeam = "0.4"
```

## Feature Flags

Moonbeam is configurable via Cargo features. Most users will want the `default` features.

- `default`: Enables `macros`, `assets`, `catchpanic`, `signals`, and `router`.
- `macros`: Enables the `#[server]` attribute macro to easily create `Server` trait implementations.
- `assets`: Exposes the `moonbeam::assets` module for serving static files.
- `signals`: Hooks into OS signals (SIGINT, SIGTERM) to trigger graceful server shutdown.
- `catchpanic`: Wraps your handlers to catch panics gracefully and return `500 Internal Server Error`.
- `tracing`: Instruments the core server loop with `tracing` spans and events.
- `compress`: Enables automatic response compression. (Depends on `flate2` and `brotli`).
- `router`: Enables the routing macros (`#[route]`, `#[middleware]`, and `router!`).
- `mt`: Exposes `serve_multi` to run multiple independent server isolates across available CPU cores.

## Configuration

Moonbeam honors the following environment variables:

- `MOONBEAM_MAX_BODY_SIZE`: Maximum size (in Kilobytes) of an incoming HTTP request body. Defaults to `1024` (1MB). Exceeding this returns a `413 Content Too Large`.

## Examples

### 1. Stateless Server

The simplest way to use Moonbeam.

```rust,no_run
use moonbeam::{Body, Request, Response, server};

#[server(HelloWorld)]
async fn serve(_request: Request) -> Response {
    Response::ok().with_body("Hello, World!", Body::TEXT)
}

fn main() {
    println!("Running on 127.0.0.1:8080");
    moonbeam::serve("127.0.0.1:8080", HelloWorld);
}
```

### 2. Stateful Server (Interior Mutability)

Because the executor runs locally, you can use `std::cell::Cell` without `Mutex`.

```rust,no_run
use std::cell::Cell;
use moonbeam::{Body, Request, Response, server};

struct AppState {
    count: Cell<u64>,
}

#[server(CounterServer)]
async fn serve(_req: Request, state: &'static AppState) -> Response {
    let count = state.count.get();
    state.count.set(count + 1);
    
    Response::ok().with_body(format!("Request #{}", count), Body::TEXT)
}

fn main() {
    let state = AppState { count: Cell::new(0) };
    moonbeam::serve("127.0.0.1:8080", CounterServer(state));
}
```

### 3. Multi-threaded "Share-Nothing" Server

Use the `mt` feature flag to scale across multiple CPU cores.

```rust,no_run
use moonbeam::{Request, Response, ThreadCount, Body, server, serve_multi};
use std::sync::atomic::{AtomicUsize, Ordering};

struct WorkerState {
    thread_id: usize,
}

#[server(Worker)]
async fn serve(_req: Request, state: &WorkerState) -> Response {
    Response::ok().with_body(format!("Hello from thread {}", state.thread_id), Body::TEXT)
}

fn main() {
    // Shared setup logic (runs once on the main thread)
    let next_id = AtomicUsize::new(0);

    serve_multi(
        "127.0.0.1:8080",
        ThreadCount::Default, // One thread per CPU core
        || {
            // This closure runs on each new thread to construct its local state
            let id = next_id.fetch_add(1, Ordering::Relaxed);
            Worker(WorkerState { thread_id: id })
        },
        |_| {} // Optional cleanup logic on shutdown
    );
}
```

### 4. Advanced Routing

The `router!` macro provides a clean domain-specific language for nesting routes and middleware.

```rust,no_run
use moonbeam::{Body, Request, Response, route, router, serve, middleware};
use moonbeam::router::PathParams;

struct AppState {
    api_key: String,
}

// Global Middleware
#[middleware]
async fn logger(req: Request, _state: &AppState, next: Next) -> Response {
    let start = std::time::Instant::now();
    let res = next(req).await;
    println!("{} {} - {:?}", req.method, req.url(), start.elapsed());
    res
}

// Scoped Middleware
#[middleware]
async fn require_auth(req: Request, state: &AppState, next: Next) -> Response {
    if req.find_header("X-Api-Key") == Some(state.api_key.as_bytes()) {
        next(req).await
    } else {
        Response::new_with_code(401).with_body("Unauthorized", Body::TEXT)
    }
}

// Extractor Handler
#[route]
async fn get_user(PathParams(id): PathParams<&str>) -> Response {
    Response::ok().with_body(format!("User ID: {}", id), Body::TEXT)
}

#[route]
async fn not_found() -> Response {
    Response::new_with_code(404).with_body("Not Found", Body::TEXT)
}

fn main() {
    router!(ApiRouter<AppState> {
        with logger

        "/api" => {
            with require_auth
            
            get("/users/:id") => get_user,
            
            // Unmatched /api/* routes to default 404
            _ => !
        }
        
        // Custom 404
        _ => not_found
    });

    let state = AppState { api_key: "secret".to_string() };
    serve("127.0.0.1:8080", ApiRouter::new(state));
}
```

## Serving Static Files

```rust,no_run
use moonbeam::{Request, Response, server, assets::get_asset};

#[server(StaticServer)]
async fn serve(req: Request) -> Response {
    let etag = req.find_header("If-None-Match");
    get_asset(req.path, etag, "./public").await
}
```

## License

This project is licensed under the MIT License.
