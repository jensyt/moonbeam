# Moonbeam Serde

Serde support for the Moonbeam web server.

This crate provides a `Json<T>` wrapper that allows for easy, typed JSON extraction and serialization. It is designed to work seamlessly with Moonbeam's `#[route]` macro.

## Features

- **Typed JSON Extraction**: Automatically parse request bodies into any type implementing `serde::Deserialize`.
- **Typed Form & Multipart Extraction**: Extract URL-encoded or multipart form data into structured types.
- **Zero-Copy Deserialization**: Supports borrowing data (like `&str`, `&[u8]`, or `Cow<str>`) directly from the request buffer to minimize allocations for both JSON and Form extractors.
- **File Uploads**: Native support for multipart file uploads with the `File` type.
- **Easy Responses**: Implementations for `Into<Response>` allow you to return `Json(my_struct)` directly from your handlers.
- **Trait-Based**: Built on top of Moonbeam's `FromRequest` and `FromBody` traits.

## Usage

Add `moonbeam-serde` and `serde` to your `Cargo.toml`:

```toml
[dependencies]
moonbeam = "0.7"
moonbeam-serde = "0.3"
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

### Example: Form Extraction (URL-Encoded)

```rust
use moonbeam::{Response, Body, route};
use moonbeam_serde::Form;
use serde::Deserialize;
use std::borrow::Cow;

#[derive(Deserialize)]
struct Profile<'a> {
    id: u32,
    #[serde(borrow)]
    name: Cow<'a, str>, // Supports zero-copy or percent-decoded values
    active: bool,
}

#[route]
async fn handle_profile(Form(profile): Form<Profile<'_>>) -> impl Into<Response> {
    (
        format!("Profile: {} (ID: {})", profile.name, profile.id).into(),
        Body::TEXT
    )
}
```

### Example: Multipart Form & File Uploads

```rust
use moonbeam::{Response, Body, route};
use moonbeam_serde::{Form, File};
use serde::Deserialize;

#[derive(Deserialize)]
struct Upload<'a> {
    title: &'a str,
    #[serde(borrow)]
    file: File<'a>,
}

#[route]
async fn handle_upload(Form(upload): Form<Upload<'_>>) -> impl Into<Response> {
    (
        format!(
            "Uploaded file {} (content type: {:?}, size: {} bytes) for title {}",
            upload.file.name.as_deref().unwrap_or(""),
            upload.file.content_type.as_deref(),
            upload.file.data.len(),
            upload.title
        ).into(),
        Body::TEXT
    )
}
```

## Error Handling

If the request body contains invalid JSON or does not match the expected structure, the `Json<T>` extractor will automatically return a `400 Bad Request` response.

Similarly, if the request does not contain a valid form content type or if deserialization fails, the `Form<T>` extractor will automatically return a `400 Bad Request` response.

## License

This project is licensed under the MIT License.
