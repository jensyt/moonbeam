# Todo
- Make tracing meaningful
- Rename lifetimes to be more useful
- Macros have different levels of support for including / reording parameters
- Macros make some poor assumptions about the input function signature's lifetime parameters

# Done
- ETags for assets
- HEAD requests
- Default content-type and content-length
- Implement cookies
- Implement params
- Handle panics in server
- Content encoding (gzip, brotli)
- Better routing
- Support returning `impl Into<Response>` and `Result<impl Into<Response>, impl Into<Response>>`
  from routes
- Middleware support
- Route prefixes
- Automatic support for HEAD requests -> GET handler in router
- Clean up project structure to remove example and test entries from Cargo.toml
- Method Not Allowed (405)
- Improve documentation (README and module/function documentation)
- Trait-based body parsing (`FromRequest`, `FromBody`)
- JSON body extraction with `moonbeam-serde` and `Json<T>`
- Support for HTML forms (urlencoded and multipart) via `moonbeam-forms` crate.
- Typed form deserialization for URL-encoded and multipart data via `moonbeam-serde`.
- Support newtype structs in moonbeam-serde forms
- Fix incorrect URL decoding before splitting
- Robust handling of URL-encoded delimiters and invalid UTF-8 in form data
- State extractors require passing a state parameter to a route or adding a generic (abusing Rust syntax)
