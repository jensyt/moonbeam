use crate::http::{cookies::Cookies, params::Params};
use httparse::{Header, Request as RawRequest};
use percent_encoding::percent_decode_str;
use std::{borrow::Cow, fmt::Debug, io::Read};

pub mod cookies;
pub mod params;

/// Returns the canonical reason phrase for a given HTTP status code.
///
/// # Example
/// ```
/// use moonbeam::http::canonical_reason;
///
/// assert_eq!(canonical_reason(200), "OK");
/// assert_eq!(canonical_reason(404), "Not Found");
/// ```
pub fn canonical_reason(code: u16) -> &'static str {
	match code {
		200 => "OK",
		204 => "No Content",
		304 => "Not Modified",
		307 => "Temporary Redirect",
		400 => "Bad Request",
		401 => "Unauthorized",
		403 => "Forbidden",
		404 => "Not Found",
		408 => "Request Timeout",
		413 => "Content Too Large",
		431 => "Request Header Fields Too Large",
		500 => "Internal Server Error",
		_ => "",
	}
}

/// Represents an HTTP request.
///
/// # Example
/// ```
/// use moonbeam::http::{Request, Body};
/// use httparse::Header;
///
/// let headers = [Header { name: "Host", value: b"localhost" }];
/// let req = Request::new("GET", "/", &headers, b"");
///
/// assert_eq!(req.method, "GET");
/// assert_eq!(req.path, "/");
/// ```
#[derive(Clone, Copy, Debug)]
pub struct Request<'headers, 'buf> {
	/// The HTTP method (e.g., "GET", "POST").
	pub method: &'buf str,
	/// The request path (e.g., "/index.html").
	pub path: &'buf str,
	/// The HTTP version (0 for 1.0, 1 for 1.1).
	pub version: u8,
	/// The request headers.
	pub headers: &'headers [Header<'buf>],
	/// The request body.
	pub body: &'buf [u8],
}

impl<'headers, 'buf> Request<'headers, 'buf> {
	/// Creates a new `Request` from a raw `httparse::Request`.
	pub fn new_from_raw(raw: RawRequest<'headers, 'buf>, body: &'buf [u8]) -> Self {
		Self {
			method: raw.method.unwrap(),
			path: raw.path.unwrap(),
			version: raw.version.unwrap(),
			headers: raw.headers,
			body,
		}
	}

	/// Creates a new `Request`.
	pub fn new(
		method: &'buf str,
		path: &'buf str,
		headers: &'headers [Header<'buf>],
		body: &'buf [u8],
	) -> Self {
		Self {
			method,
			path,
			version: 1,
			headers,
			body,
		}
	}

	/// Finds a header by name.
	#[inline]
	pub fn find_header(&self, name: &str) -> Option<&'headers [u8]> {
		self.headers
			.iter()
			.find(|h| h.name.eq_ignore_ascii_case(name))
			.map(|h| h.value)
	}

	/// Returns a helper to parse cookies from the request.
	#[inline]
	pub fn cookies(&self) -> Cookies<'headers> {
		Cookies::new(self.find_header("Cookie"))
	}

	/// Returns a helper to parse query parameters from the request URL.
	///
	/// The query parameters are URL-decoded, including converting '+' to space.
	#[inline]
	pub fn params(&self) -> Params<'buf> {
		match self.path.split('?').nth(1) {
			Some(p) => {
				let decoded = if p.contains('+') {
					let replaced = p.replace('+', " ");
					match percent_decode_str(&replaced).decode_utf8_lossy() {
						Cow::Owned(s) => Cow::Owned(s),
						Cow::Borrowed(_) => Cow::Owned(replaced),
					}
				} else {
					percent_decode_str(p).decode_utf8_lossy()
				};
				Params::new(decoded)
			}
			None => Params::new(Cow::Borrowed("")),
		}
	}

	/// Returns the decoded URL path without query parameters.
	#[inline]
	pub fn url(&self) -> Cow<'buf, str> {
		let url = self.path.split('?').next().unwrap_or(self.path);
		percent_decode_str(url).decode_utf8_lossy()
	}
}

/// Represents the collection of HTTP headers.
#[derive(Debug, Clone, Default)]
pub struct Headers {
	inner: Vec<(Cow<'static, str>, Cow<'static, str>)>,
}

impl Headers {
	/// Creates a new empty headers collection.
	pub fn new() -> Self {
		Self::default()
	}

	/// Adds a header.
	pub fn push(
		&mut self,
		name: impl Into<Cow<'static, str>>,
		value: impl Into<Cow<'static, str>>,
	) {
		self.inner.push((name.into(), value.into()));
	}

	/// Iterates over the headers.
	pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
		self.inner.iter().map(|(k, v)| (k.as_ref(), v.as_ref()))
	}

	/// Retains only the elements specified by the predicate.
	pub fn retain<F>(&mut self, mut f: F)
	where
		F: FnMut(&str, &str) -> bool,
	{
		self.inner.retain(|(k, v)| f(k, v));
	}

	/// Returns the number of headers.
	pub fn len(&self) -> usize {
		self.inner.len()
	}

	/// Returns true if there are no headers.
	pub fn is_empty(&self) -> bool {
		self.inner.is_empty()
	}
}

impl std::ops::Index<usize> for Headers {
	type Output = (Cow<'static, str>, Cow<'static, str>);

	fn index(&self, index: usize) -> &Self::Output {
		&self.inner[index]
	}
}

/// Type alias for MIME types (Content-Type), using Copy-on-Write strings.
pub type MimeType = Cow<'static, str>;

/// Represents an HTTP response.
///
/// # Example
/// ```
/// use moonbeam::http::{Response, Body};
///
/// let resp = Response::ok().with_body("Hello", Body::TEXT);
/// assert_eq!(resp.status, 200);
/// ```
#[derive(Debug)]
pub struct Response {
	/// The HTTP status code (e.g., 200, 404).
	pub status: u16,
	/// The response headers.
	pub headers: Headers,
	/// The response body.
	pub body: Option<Body>,
}

impl Response {
	/// Creates a new response with the given status code.
	#[inline]
	pub fn new_with_code(status: u16) -> Self {
		Self {
			status,
			headers: Headers::new(),
			body: None,
		}
	}

	/// Creates a new response with a body and optional content type.
	pub fn new_with_body(
		body: impl Into<Body>,
		content_type: Option<impl Into<Cow<'static, str>>>,
	) -> Self {
		let headers = if let Some(content_type) = content_type {
			Headers {
				inner: vec![("Content-Type".into(), content_type.into())],
			}
		} else {
			Headers::new()
		};
		Self {
			status: 200,
			headers,
			body: Some(body.into()),
		}
	}

	/// 204 No Content
	#[inline]
	pub fn empty() -> Self {
		Self::new_with_code(204)
	}

	/// 200 OK
	#[inline]
	pub fn ok() -> Self {
		Self::new_with_code(200)
	}

	/// 304 Not Modified
	#[inline]
	pub fn not_modified(content_type: Option<impl Into<Cow<'static, str>>>) -> Self {
		let mut resp = Self::new_with_code(304);
		if let Some(ct) = content_type {
			resp.set_header("Content-Type", ct);
		}
		resp
	}

	/// 307 Temporary Redirect
	///
	/// Sets the `Location` header to the given location.
	#[inline]
	pub fn temporary_redirect(location: impl Into<Cow<'static, str>>) -> Self {
		Self::new_with_code(307).with_header("Location", location)
	}

	/// 404 Not Found
	#[inline]
	pub fn not_found() -> Self {
		Self::new_with_code(404)
	}

	/// 500 Internal Server Error
	#[inline]
	pub fn internal_server_error() -> Self {
		Self::new_with_code(500)
	}

	/// 400 Bad Request
	#[inline]
	pub fn bad_request() -> Self {
		Self::new_with_code(400)
	}

	/// 401 Unauthorized
	#[inline]
	pub fn unauthorized() -> Self {
		Self::new_with_code(401)
	}

	/// 403 Forbidden
	#[inline]
	pub fn forbidden() -> Self {
		Self::new_with_code(403)
	}

	/// 408 Request Timeout
	#[inline]
	pub fn request_timeout() -> Self {
		Self::new_with_code(408)
	}

	/// 413 Content Too Large
	#[inline]
	pub fn content_too_large() -> Self {
		Self::new_with_code(413)
	}

	/// 431 Request Header Fields Too Large
	#[inline]
	pub fn headers_too_large() -> Self {
		Self::new_with_code(431)
	}

	/// Sets the response body and optionally the Content-Type header.
	///
	/// This overwrites the body and `Content-Type` header if they already exist.
	#[inline]
	pub fn with_body(
		mut self,
		body: impl Into<Body>,
		content_type: Option<impl Into<Cow<'static, str>>>,
	) -> Self {
		match content_type {
			Some(v) => {
				self.set_header("Content-Type", v);
			}
			None => {
				self.headers
					.retain(|n, _| !n.eq_ignore_ascii_case("Content-Type"));
			}
		}
		self.body = Some(body.into());
		self
	}

	/// Adds a header if it doesn't already exist.
	#[inline]
	pub fn with_header(
		mut self,
		name: impl Into<Cow<'static, str>>,
		value: impl Into<Cow<'static, str>>,
	) -> Self {
		let name = name.into();
		if !self
			.headers
			.iter()
			.any(|(n, _)| n.eq_ignore_ascii_case(name.as_ref()))
		{
			self.headers.push(name, value);
		}
		self
	}

	/// Sets a header, replacing any existing value. Returns the old value if it existed.
	///
	/// If multiple headers with the same name exist, they are all removed and replaced
	/// by the new single header. The value of the first removed header is returned.
	pub fn set_header(
		&mut self,
		name: impl Into<Cow<'static, str>>,
		value: impl Into<Cow<'static, str>>,
	) -> Option<Cow<'static, str>> {
		let name = name.into();
		let old_value = self
			.headers
			.inner
			.iter()
			.find(|(n, _)| n.eq_ignore_ascii_case(name.as_ref()))
			.map(|(_, v)| v.clone());

		if old_value.is_some() {
			self.headers
				.retain(|n, _| !n.eq_ignore_ascii_case(name.as_ref()));
		}

		self.headers.push(name, value);
		old_value
	}
}

/// Represents the body of an HTTP response.
///
/// # Example
/// ```
/// use moonbeam::http::Body;
///
/// let body = Body::from("Hello");
/// ```
pub enum Body {
	/// In-memory body data.
	Immediate(Vec<u8>),
	Stream {
		data: Box<dyn Read + Send + 'static>,
		/// The length of the body, if known.
		len: Option<u64>,
	},
}

impl Body {
	/// Content-Type for HTML.
	pub const HTML: Option<&'static str> = Some("text/html; charset=utf-8");
	/// Content-Type for JSON.
	pub const JSON: Option<&'static str> = Some("application/json");
	/// Content-Type for Text.
	pub const TEXT: Option<&'static str> = Some("text/plain; charset=utf-8");
	/// Default Content-Type.
	pub const DEFAULT_CONTENT_TYPE: Option<&'static str> = None;

	/// Creates a body from a vector.
	pub fn from_vec(data: impl Into<Vec<u8>>) -> Self {
		Self::Immediate(data.into())
	}

	#[allow(clippy::len_without_is_empty)]
	pub fn len(&self) -> Option<u64> {
		match self {
			Body::Immediate(data) => Some(data.len() as u64),
			Body::Stream { len, .. } => *len,
		}
	}
}

impl From<Vec<u8>> for Body {
	fn from(data: Vec<u8>) -> Self {
		Body::Immediate(data)
	}
}

impl From<String> for Body {
	fn from(data: String) -> Self {
		Body::Immediate(data.into_bytes())
	}
}

impl From<&str> for Body {
	fn from(data: &str) -> Self {
		Body::Immediate(data.as_bytes().to_vec())
	}
}

impl From<&[u8]> for Body {
	fn from(data: &[u8]) -> Self {
		Body::Immediate(data.to_vec())
	}
}

impl From<Box<[u8]>> for Body {
	fn from(data: Box<[u8]>) -> Self {
		Body::Immediate(data.into())
	}
}

impl From<std::fs::File> for Body {
	fn from(file: std::fs::File) -> Self {
		let len = file.metadata().map(|meta| meta.len()).ok();
		Body::Stream {
			data: Box::new(file),
			len,
		}
	}
}

impl Debug for Body {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Immediate(v) => write!(f, "Body(Immediate, len={})", v.len()),
			Self::Stream { data: _, len } => write!(f, "Body(Stream, len={len:?})"),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_response_add_header() {
		let response = Response {
			status: 200,
			headers: Headers::new(),
			body: None,
		};

		let response = response.with_header("Content-Type", "text/html");
		assert_eq!(response.headers.len(), 1);
		assert_eq!(response.headers[0].0, "Content-Type");
		assert_eq!(response.headers[0].1, "text/html");

		// Adding same header should not duplicate
		let response = response.with_header("content-type", "application/json");
		assert_eq!(response.headers.len(), 1);
		assert_eq!(response.headers[0].0, "Content-Type");
		assert_eq!(response.headers[0].1, "text/html");

		// Adding different header should work
		let response = response.with_header("X-Custom", "value");
		assert_eq!(response.headers.len(), 2);
	}

	#[test]
	fn test_response_set_header() {
		let mut response = Response {
			status: 200,
			headers: Headers {
				inner: vec![("Content-Type".into(), "text/html".into())],
			},
			body: None,
		};

		// Setting existing header should replace
		let old_value = response.set_header("Content-Type", "application/json");
		assert_eq!(old_value, Some(Cow::Borrowed("text/html")));
		assert_eq!(response.headers[0].1, "application/json");

		// Setting new header should add
		let old_value = response.set_header("X-New", "new-value");
		assert_eq!(old_value, None);
		assert_eq!(response.headers.len(), 2);
	}

	#[test]
	fn test_response_set_header_removes_duplicates() {
		let mut response = Response {
			status: 200,
			headers: Headers {
				inner: vec![
					("X-Custom".into(), "v1".into()),
					("X-Custom".into(), "v2".into()),
				],
			},
			body: None,
		};

		let old_value = response.set_header("X-Custom", "v3");
		assert_eq!(old_value, Some(Cow::Borrowed("v1")));
		assert_eq!(response.headers.len(), 1);
		assert_eq!(response.headers[0].0, "X-Custom");
		assert_eq!(response.headers[0].1, "v3");
	}

	#[test]
	fn test_request_find_header() {
		let headers = [
			Header {
				name: "Content-Type",
				value: b"text/plain",
			},
			Header {
				name: "X-Custom",
				value: b"custom",
			},
		];
		let req = Request::new("GET", "/", &headers, &[]);

		assert_eq!(
			req.find_header("Content-Type"),
			Some(b"text/plain" as &[u8])
		);
		assert_eq!(
			req.find_header("content-type"),
			Some(b"text/plain" as &[u8])
		);
		assert_eq!(req.find_header("X-Custom"), Some(b"custom" as &[u8]));
		assert_eq!(req.find_header("x-custom"), Some(b"custom" as &[u8]));
		assert_eq!(req.find_header("NotFound"), None);
	}

	#[test]
	fn test_request_url() {
		let headers = [];
		let req = Request::new("GET", "/path?query=1", &headers, &[]);
		assert_eq!(req.url(), "/path");

		let req = Request::new("GET", "/path", &headers, &[]);
		assert_eq!(req.url(), "/path");

		let req = Request::new("GET", "/path%20space", &headers, &[]);
		assert_eq!(req.url(), "/path space");

		let req = Request::new("GET", "/path+plus", &headers, &[]);
		assert_eq!(req.url(), "/path+plus");
	}

	#[test]
	fn test_response_constructors() {
		let r = Response::ok();
		assert_eq!(r.status, 200);

		let r = Response::not_found();
		assert_eq!(r.status, 404);

		let r = Response::bad_request();
		assert_eq!(r.status, 400);

		let r = Response::temporary_redirect("/foo");
		assert_eq!(r.status, 307);
		assert_eq!(r.headers[0].0, "Location");
		assert_eq!(r.headers[0].1, "/foo");
	}

	#[test]
	fn test_response_with_body() {
		let r = Response::ok().with_body("hello", Some("text/plain"));
		assert_eq!(r.status, 200);
		assert!(r.body.is_some());
		assert_eq!(
			r.headers
				.iter()
				.find(|&(n, _)| n == "Content-Type")
				.unwrap()
				.1,
			"text/plain"
		);

		// Test replacing content type
		let r = r.with_body("world", Some("application/json"));
		assert_eq!(
			r.headers
				.iter()
				.find(|&(n, _)| n == "Content-Type")
				.unwrap()
				.1,
			"application/json"
		);
		assert_eq!(
			r.headers
				.iter()
				.filter(|&(n, _)| n == "Content-Type")
				.count(),
			1
		);

		// Test removing content type
		let r = r.with_body("data", Body::DEFAULT_CONTENT_TYPE);
		assert!(!r.headers.iter().any(|(n, _)| n == "Content-Type"));
	}

	#[test]
	fn test_request_params_plus_decoding() {
		let headers = [];
		let req = Request::new("GET", "/?a=b+c&d=e%20f&g=%2B", &headers, &[]);
		let params = req.params();
		assert_eq!(params.find("a").next(), Some("b c"));
		assert_eq!(params.find("d").next(), Some("e f"));
		assert_eq!(params.find("g").next(), Some("+"));
	}
}
