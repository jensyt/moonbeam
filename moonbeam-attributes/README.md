# Moonbeam Attributes

This crate provides procedural macros for the `moonbeam` web server library.
The main macro is `#[server]`, which simplifies creating server implementations by wrapping a function.
With the `router` feature, this crate also offers the `#[route]`, `router!`, and `#[middleware]` macros which provide a clean DSL and efficient implementation for nested groups, middleware, path parameters, and wildcards. It also enables the `#[from_request]` macro, which simplifies custom request extractors using `FromBody` or `FromState`.

## Usage

### `#[server]`

The `#[server]` attribute macro converts a function into a struct that implements the `Server` trait. This struct can then be passed to the `moonbeam::serve` function.

#### Arguments

* `name`: The name of the struct to generate.

#### Function Signature

The decorated function must have one of the following signatures:

*   `async fn(Request, Spawner) -> Response`
*   `async fn(Request, Spawner, &State) -> Response` (if state is used)

The function can be `async` or return `impl Future<Output = Response>`.

#### Examples

**Stateless Server**

```rust
use moonbeam::{Body, Request, Response, Spawner, server, serve};

#[server(MyServer)]
async fn handle_request(_req: Request, _spawner: Spawner) -> Response {
    Response::ok().with_body("Hello, World!", Body::TEXT)
}

fn main() {
    serve("127.0.0.1:8080", || MyServer);
}
```

**Stateful Server**

```rust
use moonbeam::{Body, Request, Response, Spawner, server, serve};
use std::sync::atomic::{AtomicUsize, Ordering};

struct State {
    count: AtomicUsize,
}

#[server(MyStatefulServer)]
async fn handle_request(_req: Request, _spawner: Spawner, state: &State) -> Response {
    let count = state.count.fetch_add(1, Ordering::Relaxed);
    Response::ok().with_body(format!("Request count: {}", count), Body::TEXT)
}

fn main() {
    let state = State {
        count: AtomicUsize::new(0),
    };
    // The macro generates a tuple struct wrapper.
    // Pass the state to the generated struct constructor.
    serve("127.0.0.1:8080", move || MyStatefulServer(state));
}
```

### `#[route]`

The `#[route]` macro defines a route handler for use within a `router!`. It provides powerful dependency injection by automatically extracting arguments based on the function signature.

#### Supported Parameters

*   **`Request`**: The raw HTTP request.
*   **`Spawner`**: A handle for spawning asynchronous tasks.
*   **`&State`**: A reference to the application state.
*   **`PathParams<T>`**: Extracted path parameters (e.g., `PathParams<&str>`).
*   **Extractors**: Any type implementing `FromRequest`. This allows for typed body parsing, such as `Json<T>`.

#### Flexible Return Types

The decorated function can return any type that implements `Into<Response>`. This includes:
*   `Response`
*   `()` (returns `204 No Content`)
*   `Body` or `String` (returns `200 OK`)
*   `Result<T, E>` where both `T` and `E` implement `Into<Response>`.
*   Tuples like `(Body, &'static str)` to specify `Content-Type`.

#### Example

```rust
use moonbeam::{Body, Response, route};
use moonbeam::router::PathParams;
use moonbeam_serde::Json;
use serde::Deserialize;

#[derive(Deserialize)]
struct User<'a> {
    name: &'a str,
}

#[route]
async fn hello_user(
    PathParams(id): PathParams<u32>,
    Json(user): Json<User<'_>>
) -> Result<String, Response> {
    if id == 0 {
        return Err(Response::bad_request());
    }
    Ok(format!("Hello, {} (ID: {})!", user.name, id))
}
```

### `#[middleware]`

The `#[middleware]` macro simplifies the creation of middleware. It injects the necessary lifetimes and types for the `Request` and the `Next` function.

#### Example

```rust
use moonbeam::{Request, Response, middleware, Spawner};

#[middleware]
async fn logger(req: Request, spawner: Spawner, state: &AppState, next: Next) -> Response {
    println!("Request: {} {}", req.method, req.path);
    next(req).await
}
```

### `router!`

The `router!` macro provides a declarative DSL for defining complex routing trees, including nesting, middleware, and fallbacks.

#### Example

```rust
router!(MyRouter<AppState> {
    // Apply global middleware
    with logger

    // Define routes
    get("/") => index_handler,
    get("/hello/:name") => hello_user,

    // Nested groups with prefixes
    "/api" => {
        with auth_middleware
        get("/data") => data_handler
    }

    // Fallback route
    _ => not_found_handler
});
```

### `#[from_request]`

The `#[from_request]` attribute macro simplifies the implementation of custom extractors. Instead of implementing the full `FromRequest` trait directly, you can implement the simpler `FromBody` or `FromState` traits and decorate the implementation block with `#[from_request]`. The macro automatically generates the corresponding `FromRequest` implementation.

#### Implementing `FromBody`

Use `FromBody` when you want to extract a value from the raw request body.

```rust
use moonbeam::{Response, from_request};
use moonbeam::http::FromBody;

struct Name<'a>(&'a str);

#[from_request]
impl<'b> FromBody<'b> for Name<'b> {
    type Error = Response<'static>;

    fn from_body(body: &'b [u8]) -> Result<Self, Self::Error> {
        str::from_utf8(body)
            .map(Name)
            .map_err(|_| Response::bad_request())
    }
}
```

#### Implementing `FromState`

Use `FromState` when you want to extract/reference a specific part of the application state.

```rust
use moonbeam::{Response, from_request};
use moonbeam::http::FromState;
use std::convert::Infallible;

struct Database {
    // ...
}

struct AppState {
    db: Database,
}

// A custom extractor that references the Database within AppState
struct DbConn<'a>(&'a Database);

#[from_request]
impl<'s> FromState<'s, AppState> for DbConn<'s> {
    type Error = Infallible;

    fn from_state(state: &'s AppState) -> Result<Self, Self::Error> {
        Ok(Self(&state.db))
    }
}
```

> [!NOTE]
> If you use an extractor implemented via `FromState` (which is bound to a specific state type), you must specify the state type explicitly on the route handler using `#[route(state = AppState)]`.
