# Moonbeam Project Context for Agents

## Project Overview
**Moonbeam** is a single-threaded-first, asynchronous HTTP server written in Rust. It prioritizes simplicity and performance by running on a single thread by default, leveraging the `async-io` and `smol` ecosystem. It avoids the complexity of thread synchronization primitives like `Arc` and `Mutex` in favor of a "share-nothing" architecture.

## Tech Stack & Dependencies
- **Runtime**: `async-executor` (LocalExecutor), `async-io`, `futures-lite`.
- **Networking**: `async-net`.
- **Parsing**: `httparse`.
- **Utilities**: `blocking` (for synchronous I/O), `flume` (async-ready channels), `percent-encoding`.
- **Compression**: `flate2` (gzip), `brotli`.
- **Testing**: `piper` (for mock async streams).
- **Log**: `tracing`.

**CRITICAL**: This project does **NOT** use `tokio`. Do not introduce `tokio` dependencies, `tokio::spawn`, or `#[tokio::main]`. Use `async_io::block_on` or the project's own `serve`/`serve_multi` functions.

## Core Philosophy
1.  **Single-Threaded by Default**: Uses a `LocalExecutor` on the main thread. State can be stored in `RefCell` or `Cell` as no thread-sharing is required.
2.  **Share-Nothing Multi-Threading**: The `mt` feature enables multi-threading by spawning independent server "isolates" on worker threads. Each thread leaks its own copy of the state to create a `'static` reference.
3.  **Macro-Driven DSL**: Heavy use of procedural macros (`router!`, `#[route]`) to eliminate boilerplate and provide dependency injection for handlers.

## Workspace Structure
- **`moonbeam/`**: Core library.
    - `src/server/st.rs`: Single-threaded runtime implementation.
    - `src/server/mt.rs`: Multi-threaded runtime (requires `mt` feature).
    - `src/router/`: Routing logic and `PathParams` extraction.
    - `src/http/`: `Request`, `Response`, `Body`, `Cookies`, and `Params` (query strings).
    - `src/assets.rs`: Static file serving with ETag (SHA-based) and MIME detection.
- **`moonbeam-attributes/`**: Procedural macros (`router!`, `#[server]`, `#[route]`, `#[middleware]`).

## Key Components

### Routing DSL (`router!`)
The `router!` macro defines the routing tree.
```rust
router!(MyRouter<State> {
    with logger_middleware // Global middleware

    get("/") => index_handler,
    get("/users/:id") => user_handler, // Extracted via PathParams

    "/api" => {
        with auth_middleware
        post("/submit") => submit_handler,
        _ => ! // 404 for this sub-tree
    }
    _ => not_found_handler // Global 404
});
```

### Handlers & State
Handlers are async functions. The `#[route]` macro allows them to automatically extract data from the request. Supported arguments include:
- `Request`: The raw request object.
- `&State`: A reference to the application state (must be a reference).
- `PathParams<(T1, T2, ...)>`: Extracted path variables.
- **Extractors**: Any type implementing `FromRequest`. This allows for flexible, typed body extraction (e.g., `Json<T>`).

#### Custom Extractors
Implement `FromRequest` or `FromBody` in `moonbeam/src/http/mod.rs` to create custom extractors. `FromBody` provides a blanket implementation of `FromRequest` for types that only need the raw body bytes.

- **`moonbeam-serde`**: A separate crate providing `Json<T>` for automatic JSON parsing using `serde_json`.

Handlers can return anything that implements `Into<Response>`, including `Result<T, E>` where both `T` and `E` are `Into<Response>`.

### Middleware
Middleware signatures are simplified via `#[middleware]`:
```rust
#[middleware]
async fn my_middleware(req: Request, state: &State, next: Next) -> Response {
    // next(req) returns a Future<Output = Response>
    next(req).await
}
```

## Development Guidelines
- **Interior Mutability**: Use `std::rc::Rc` and `std::cell::RefCell` for state. Avoid `std::sync` unless explicitly required for cross-thread channels (`flume`).
- **Memory Management**: Server instances are typically boxed and leaked (`Box::leak`) to provide the `'static` lifetime required by the executor.
- **Error Handling**: Prefer returning `Response::internal_server_error()` or similar over panicking. The `catchpanic` feature (if enabled) will catch panics in handlers and return a 500 response.

## Testing Strategy
- **Unit Tests**: Use `piper::pipe` to create connected `Reader`/`Writer` pairs to simulate sockets.
- **Mocking**: Handlers can be tested by manually constructing `Request` objects and calling `handler(req, state).await`.

## Development Workflow
- **Formatting**: Always format code using `cargo fmt`.
- **Testing**: Run tests with `cargo test`. To ensure all features are covered, use `cargo test --all-features`.
- **Documentation**: Generate and view documentation with `cargo doc --open`.
- **Linting**: Use `cargo clippy` to check for idiomatic Rust code.