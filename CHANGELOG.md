# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- TLS support via `rustls` (behind the `tls` cargo feature). Exposes `serve_tls` and `serve_multi_tls` functions for starting single-threaded or multi-threaded HTTPS servers.
- `moonbeam::TlsConfig` helper to load certificates and private keys from PEM files.

## [0.7.0] - 2026-05-21

### Added
- `Spawner` and `Executor` types in `moonbeam::server::task` to manage asynchronous task execution without global state.
- Handlers can now take `Spawner` as an argument to spawn tasks that live as long as the server or less.
- The `#[route]` macro now supports an explicit `state` argument (e.g., `#[route(state = AppState)]`) to support state extractors.

### Changed
- **BREAKING**: Removed the `'static` lifetime requirement from the `Server` trait.
- **BREAKING**: The `Server::route` method now accepts a `Spawner` argument.
- **BREAKING**: `moonbeam::serve` now accepts a factory closure instead of a server instance, matching the signature of `serve_multi`.
- **BREAKING**: `moonbeam::serve` and `moonbeam::serve_multi` no longer require the application state to be `'static`. They no longer leak the server state using `Box::leak`.
- **BREAKING**: `FromRequest` trait now includes an additional lifetime parameter `'s` for the state reference.
- **BREAKING**: `RouteHandler::call` now includes `Spawner` and has updated lifetime bounds.

## [0.6.0] - 2026-05-10

### Added
- Added an iterator for percent-decoded URL path segments (`PathIterator`).
- Added `PercentDecode` and `PercentDecodeExt` for streamlined URL decoding of strings and iterators.

### Changed
- **BREAKING**: `moonbeam::http::Request` now separates the `path` and `query` components. Use `req.path` for the raw path (without query string) and `req.query` for the raw query string.
- **BREAKING**: `moonbeam::http::params::Params` and `moonbeam-forms` now return `Cow<'a, str>` instead of `&str`. This enables proper percent-decoding of values that require allocation (e.g., those containing `+` or `%20`).
- Handle invalid UTF-8 in form data by using lossy decoding in `moonbeam-serde` rather than skipping.

### Fixed
- The above changes were necessary to fix a bug where URL-encoded delimiters (like `%26` for `&`) were incorrectly decoded before splitting path segments and query parameters.

## [0.5.2] - 2026-04-10

### Added
- Support for newtype structs and byte arrays in `moonbeam-serde` form extraction.
- Fixed potential response splitting by sanitizing headers before writing to socket.
- Updated cookie parsing to allow multiple spaces before the name.
- Added default security headers (X-Content-Type-Options, Referrer-Policy).

## [0.5.1] - 2026-04-05

### Changed
- Updated dependency version for `moonbeam-attributes` to `0.2` in `moonbeam` to fix the breaking changes from `0.5.0`.

## [0.5.0] - 2026-04-05

### Added
- **Trait-Based Body Parsing**: Introduced `FromRequest` and `FromBody` traits for flexible, typed request body extraction.
- **Built-in Extractors**: Implemented `FromRequest` for `Params` and `Cookies` in the core `moonbeam` library.
- **Macro Enhancements**: Updated the `#[route]` macro to support asynchronous argument extraction and improved type inference for handler return values.
- **Zero-Copy JSON Support**: New `moonbeam-serde` crate providing a `Json<T>` extractor with support for zero-copy deserialization.
- **HTML Form Support**: New `moonbeam-forms` crate for parsing `application/x-www-form-urlencoded` and `multipart/form-data` (including file uploads).
- **Typed Form Support**: `Form<T>` extractor in `moonbeam-serde` for typed deserialization of URL-encoded and multipart form data, including automatic string-to-number/bool coercion.

### Changed
- **BREAKING**: `moonbeam::http::params::Params::new` now takes `&str` instead of `Cow<'a, str>` and handles percent-decoding internally. Added `into_inner()` to retrieve the decoded string.
- **BREAKING**: `RouteHandler::call` now returns `Response` directly instead of `impl Into<Response>`. This is handled automatically by the `#[route]` macro and only affects manual trait implementations.
- **Fixed Lifetimes**: `Request::find_header` and `Request::cookies` now return references tied to the request buffer (`'buf`) rather than the headers array (`'headers`).
- **Infallible Response**: Added `From<Infallible> for Response` to simplify handlers using infallible extractors.

## [0.4.0] - 2026-03-27

### Changed
- **BREAKING**: `moonbeam::assets::get_asset` is now an `async` function.
- `get_asset` now offloads all blocking filesystem operations to a background thread pool, preventing the main executor from stalling and improving throughput for single-threaded mode by ~66%.

### Added
- "Small file" optimization in `get_asset`: Files under 16KB are now read entirely into memory immediately to avoid streaming overhead.
- Basic performance benchmarks in the `README.md` comparing Moonbeam to Node.js and Rouille.

## [0.3.6] - 2026-03-24

### Added
- **Documentation**: Enhanced `docs.rs` integration. Added `doc_cfg` attributes to all feature-gated APIs, allowing users to see required features directly in the documentation.
- **Internal**: Bumped `moonbeam-attributes` to `v0.1.3` with matching `docs.rs` improvements.

## [0.3.5] - 2026-03-23

### Added
- Initial open source documentation structure.
- Comprehensive `README.md` with motivation and caveats.
- `LICENSE` (MIT).
- `CONTRIBUTING.md`.
- `SECURITY.md` for vulnerability reporting.
- Doc comments for all public APIs in `moonbeam` and `moonbeam-attributes`.
- Examples for stateless, stateful, and multi-threaded server configurations.
