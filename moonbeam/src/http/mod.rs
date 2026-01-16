use crate::http::{cookies::Cookies, params::Params};
use httparse::{Header, Request as RawRequest};
use percent_encoding::percent_decode_str;
use std::{
	borrow::{Borrow, Cow},
	fmt::Debug,
	io::Read,
};

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
	/// The query parameters are URL-decoded.
	#[inline]
	pub fn params(&self) -> Params<'headers> {
		match self.path.split('?').nth(1) {
			Some(p) => Params::new(percent_decode_str(p).decode_utf8_lossy()),
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

/// Represents an HTTP response.
///
/// # Example
/// ```
/// use moonbeam::http::Response;
///
/// let resp = Response::ok().with_body("Hello", None);
/// assert_eq!(resp.status, 200);
/// ```
#[derive(Debug)]
pub struct Response {
	/// The HTTP status code (e.g., 200, 404).
	pub status: u16,
	/// The response headers.
	pub headers: Vec<(String, String)>,
	/// The response body.
	pub body: Option<Body>,
}

impl Response {
	/// Creates a new response with the given status code.
	#[inline]
	pub fn new_with_code(status: u16) -> Self {
		Self {
			status,
			headers: Vec::new(),
			body: None,
		}
	}

	/// Creates a new response with a body and optional content type.
	pub fn new_with_body(body: impl Into<Body>, content_type: Option<&str>) -> Self {
		let headers = if let Some(c) = content_type {
			vec![("Content-Type".to_string(), c.to_string())]
		} else {
			Vec::new()
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
	pub fn not_modified(content_type: Option<&str>) -> Self {
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
	pub fn temporary_redirect(location: impl Into<String>) -> Self {
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
	pub fn with_body(mut self, body: impl Into<Body>, content_type: Option<&str>) -> Self {
		match content_type {
			Some(v) => {
				self.set_header("Content-Type", v);
			}
			None => {
				self.headers
					.retain(|(n, _)| !n.eq_ignore_ascii_case("Content-Type"));
			}
		}
		self.body = Some(body.into());
		self
	}

	/// Adds a header if it doesn't already exist.
	#[inline]
	pub fn with_header<H, V>(mut self, name: H, value: V) -> Self
	where
		H: Into<String> + Borrow<str>,
		V: Into<String>,
	{
		if self
			.headers
			.iter()
			.find(|(n, _)| n.eq_ignore_ascii_case(name.borrow()))
			.is_none()
		{
			self.headers.push((name.into(), value.into()));
		}
		self
	}

	/// Sets a header, replacing any existing value. Returns the old value if it existed.
	///
	/// If multiple headers with the same name exist, they are all removed and replaced
	/// by the new single header. The value of the first removed header is returned.
	pub fn set_header<H, V>(&mut self, name: H, value: V) -> Option<String>
	where
		H: Into<String> + Borrow<str>,
		V: Into<String>,
	{
		let name_str = name.borrow();
		let old_value = self
			.headers
			.iter()
			.find(|(n, _)| n.eq_ignore_ascii_case(name_str))
			.map(|(_, v)| v.clone());

		if old_value.is_some() {
			self.headers
				.retain(|(n, _)| !n.eq_ignore_ascii_case(name_str));
		}

		self.headers.push((name.into(), value.into()));
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

	/// Creates a body from a vector.
	pub fn from_vec(data: impl Into<Vec<u8>>) -> Self {
		Self::Immediate(data.into())
	}

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
			headers: vec![],
			body: None,
		};

		let response = response.with_header("Content-Type", "text/html");
		assert_eq!(response.headers.len(), 1);
		assert_eq!(
			response.headers[0],
			("Content-Type".to_string(), "text/html".to_string())
		);

		// Adding same header should not duplicate
		let response = response.with_header("content-type", "application/json");
		assert_eq!(response.headers.len(), 1);
		assert_eq!(
			response.headers[0],
			("Content-Type".to_string(), "text/html".to_string())
		);

		// Adding different header should work
		let response = response.with_header("X-Custom", "value");
		assert_eq!(response.headers.len(), 2);
	}

	#[test]
	fn test_response_set_header() {
		let mut response = Response {
			status: 200,
			headers: vec![("Content-Type".to_string(), "text/html".to_string())],
			body: None,
		};

		// Setting existing header should replace
		let old_value = response.set_header("Content-Type", "application/json");
		assert_eq!(old_value, Some("text/html".to_string()));
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
			headers: vec![
				("X-Custom".to_string(), "v1".to_string()),
				("X-Custom".to_string(), "v2".to_string()),
			],
			body: None,
		};

		let old_value = response.set_header("X-Custom", "v3");
		assert_eq!(old_value, Some("v1".to_string()));
		assert_eq!(response.headers.len(), 1);
		assert_eq!(
			response.headers[0],
			("X-Custom".to_string(), "v3".to_string())
		);
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
		assert_eq!(r.headers[0], ("Location".to_string(), "/foo".to_string()));
	}

	#[test]
	fn test_response_with_body() {
		let r = Response::ok().with_body("hello", Some("text/plain"));
		assert_eq!(r.status, 200);
		assert!(r.body.is_some());
		assert_eq!(
			r.headers
				.iter()
				.find(|(n, _)| n == "Content-Type")
				.unwrap()
				.1,
			"text/plain"
		);

		// Test replacing content type
		let r = r.with_body("world", Some("application/json"));
		assert_eq!(
			r.headers
				.iter()
				.find(|(n, _)| n == "Content-Type")
				.unwrap()
				.1,
			"application/json"
		);
		assert_eq!(
			r.headers
				.iter()
				.filter(|(n, _)| n == "Content-Type")
				.count(),
			1
		);

		// Test removing content type
		let r = r.with_body("data", None);
		assert!(
			r.headers
				.iter()
				.find(|(n, _)| n == "Content-Type")
				.is_none()
		);
	}
}
