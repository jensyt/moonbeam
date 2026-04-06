# Moonbeam Forms

HTML form and multipart support for the Moonbeam web server.

This crate provides an extractor for handling:
- `application/x-www-form-urlencoded`
- `multipart/form-data`

## Usage

### Low-level Access

For simple key-value access, you can use `moonbeam::http::params::Params` directly as an extractor, or use the [`Form`] extractor which handles both URL-encoded and multipart data transparently.

```rust,no_run
use moonbeam::{Request, Response, route, Body};
use moonbeam_forms::{Form, FormData};

#[route]
async fn handle_form(form: Form<'_>) -> Response {
    // Find a specific field (iterator for multiple values)
    let name = form.find("name")
        .next()
        .and_then(|data| {
            if let FormData::Text(s) = data { Some(s) } else { None }
        })
        .unwrap_or("stranger");

    Response::ok().with_body(format!("Hello, {}!", name), Body::TEXT)
}
```

### Typed Deserialization (Serde)

For typed deserialization, use the `moonbeam-serde` crate which provides a `Form<T>` extractor. This supports both URL-encoded and multipart forms, and can automatically coerce strings to numbers and booleans.

```rust,ignore
use moonbeam::{Response, route, Body};
use moonbeam_serde::Form;
use serde::Deserialize;
use std::borrow::Cow;

#[derive(Deserialize)]
struct User<'a> {
	#[serde(borrow)]
    name: Cow<'a, str>,
    age: u32,
}

#[route]
async fn create_user(Form(user): Form<User<'_>>) -> Response {
    Response::ok().with_body(format!("Created user {} (age {})", user.name, user.age), Body::TEXT)
}
```

### Multipart Uploads

The [`Form`] extractor also handles multipart file uploads:

```rust,no_run
use moonbeam::{Response, route, Body};
use moonbeam_forms::{Form, FormData};

#[route]
async fn upload(form: Form<'_>) -> Response {
    let mut out = String::new();
    // iter() yields all fields in the order they appear
    // Returns (Option<&str>, FormData)
    for (name, data) in form.iter() {
        match data {
            FormData::File { name: filename, content_type, data } => {
                out.push_str(&format!("Received file: {:?} ({:?}) - {} bytes\n", filename, content_type, data.len()));
            }
            FormData::Text(val) => {
                out.push_str(&format!("Received field: {:?} = {:?}\n", name, val));
            }
        }
    }
    Response::ok().with_body(out, Body::TEXT)
}
```
