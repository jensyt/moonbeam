# Moonbeam Attributes

This crate provides procedural macros for the `moonbeam` web server library.
The main macro is `#[server]`, which simplifies creating server implementations
by wrapping a function.

## Usage

### `#[server]`

The `#[server]` attribute macro converts a function into a struct that implements the `Server` trait. This struct can then be passed to the `moonbeam::serve` function.

#### Arguments

* `name`: The name of the struct to generate.

#### Function Signature

The decorated function must have one of the following signatures:

*   `fn(Request) -> impl Future<Output = Response>`
*   `fn(Request, &State) -> impl Future<Output = Response>` (if state is used)

The function can be `async` or return `impl Future`.

#### Examples

**Stateless Server**

```rust
use moonbeam::{Request, Response, server, serve};

#[server(MyServer)]
async fn handle_request(_req: Request<'_, '_>) -> Response {
    Response::ok().with_body("Hello, World!", None)
}

fn main() {
    serve("127.0.0.1:8080", MyServer);
}
```

**Stateful Server**

```rust
use moonbeam::{Request, Response, server, serve};
use std::sync::atomic::{AtomicUsize, Ordering};

struct State {
    count: AtomicUsize,
}

#[server(MyStatefulServer)]
async fn handle_request(_req: Request<'_, '_>, state: &'static State) -> Response {
    let count = state.count.fetch_add(1, Ordering::Relaxed);
    Response::ok().with_body(format!("Request count: {}", count), None)
}

fn main() {
    let state = State {
        count: AtomicUsize::new(0),
    };
    // The macro generates a tuple struct wrapper.
    // Pass the state to the generated struct constructor.
    serve("127.0.0.1:8080", MyStatefulServer(state));
}
```
