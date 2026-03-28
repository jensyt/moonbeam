# Moonbeam Serde

Serde support for the Moonbeam web server.

This crate provides a `Json<T>` wrapper that allows for easy, typed JSON extraction and serialization. It is designed to work seamlessly with Moonbeam's `#[route]` macro.

## Features

- **Typed JSON Extraction**: Automatically parse request bodies into any type implementing `serde::Deserialize`.
- **Zero-Copy Deserialization**: Supports borrowing data (like `&str` or `&[u8]`) directly from the request buffer to minimize allocations.
- **Easy Responses**: Implementations for `Into<Response>` allow you to return `Json(my_struct)` directly from your handlers.
- **Trait-Based**: Built on top of Moonbeam's `FromRequest` and `FromBody` traits.

## Usage

Add `moonbeam-serde` and `serde` to your `Cargo.toml`:

```toml
[dependencies]
moonbeam = "0.4"
moonbeam-serde = "0.1"
serde = { version = "1.0", features = ["derive"] }
```

### Example: Zero-Copy Extraction

```rust
use moonbeam::{Response, Body, route};
use moonbeam_serde::Json;
use serde::Deserialize;

#[derive(Deserialize)]
struct User<'a> {
    id: u32,
    name: &'a str, // Borrowed from the request body
}

#[route]
async fn create_user(Json(user): Json<User<'_>>) -> impl Into<Response> {
	(
		format!("Created user {} with ID {}", user.name, user.id).into(),
		Body::TEXT
	)
}
```

### Example: Returning JSON

```rust
use moonbeam::route;
use moonbeam_serde::Json;
use serde::Serialize;

#[derive(Serialize)]
struct Status {
    ok: bool,
}

#[route]
async fn get_status() -> Json<Status> {
    Json(Status { ok: true })
}
```

## Error Handling

If the request body contains invalid JSON or does not match the expected structure, the `Json<T>` extractor will automatically return a `400 Bad Request` response.

## License

This project is licensed under the MIT License.
