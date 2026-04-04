# Moonbeam Forms

HTML form and multipart support for the Moonbeam web server.

This crate provides extractors for handling:
- `application/x-www-form-urlencoded` (via `Form<T>`)
- `multipart/form-data` (via `Multipart`)

## Usage

### URL-Encoded Forms

For simple key-value access, you can use `moonbeam::http::params::Params` directly as an extractor:

```rust
use moonbeam::{Request, Response, route};
use moonbeam::http::params::Params;

#[route]
async fn handle_form(params: Params<'_>) -> Response {
    let name = params.find("name").next().unwrap_or("stranger");
    Response::ok().with_body(format!("Hello, {}!", name), None)
}
```

For typed deserialization, enable the `serde` feature and use `Form<T>`:

```rust
use moonbeam::{Response, route};
use moonbeam_forms::Form;
use serde::Deserialize;

#[derive(Deserialize)]
struct User {
    name: String,
    age: u32,
}

#[route]
async fn create_user(Form(user): Form<User>) -> Response {
    Response::ok().with_body(format!("Created user {} (age {})", user.name, user.age), None)
}
```

### Multipart Uploads

Use the `Multipart` extractor to handle file uploads:

```rust
use moonbeam::{Response, route};
use moonbeam_forms::Multipart;

#[route]
async fn upload(multipart: Multipart<'_>) -> Response {
    for part in multipart.parts() {
        if let Some(filename) = part.filename {
            println!("Received file: {} ({} bytes)", filename, part.data.len());
        } else {
            println!("Received field: {:?}", part.name);
        }
    }
    Response::ok()
}
```

## Features

- `serde`: Enables `Form<T>` for typed deserialization (uses `serde_urlencoded`).
- `tracing`: Enables logging for form parsing errors.
