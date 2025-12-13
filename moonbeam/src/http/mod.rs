use crate::http::{cookies::Cookies, params::Params};
use futures_lite::AsyncRead;
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
#[derive(Clone, Copy, Debug)]
pub struct Request<'headers, 'buf> {
	pub method: &'buf str,
	pub path: &'buf str,
	pub version: u8,
	pub headers: &'headers [Header<'buf>],
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
#[derive(Debug)]
pub struct Response {
	pub status: u16,
	pub headers: Vec<(String, String)>,
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
	pub fn not_modified() -> Self {
		Self::new_with_code(304)
	}

	/// 307 Temporary Redirect
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
	pub fn set_header<H, V>(&mut self, name: H, value: V) -> Option<String>
	where
		H: Into<String> + Borrow<str>,
		V: Into<String>,
	{
		match self
			.headers
			.iter_mut()
			.find(|(n, _)| n.eq_ignore_ascii_case(name.borrow()))
		{
			Some((_, v)) => Some(std::mem::replace(v, value.into())),
			None => {
				self.headers.push((name.into(), value.into()));
				None
			}
		}
	}
}

/// Represents the body of an HTTP response.
pub enum Body {
	/// In-memory body data.
	Immediate(Vec<u8>),
	/// Synchronous reader body.
	Sync {
		data: Box<dyn Read + 'static>,
		len: Option<u64>,
	},
	/// Asynchronous reader body.
	Async {
		data: Box<dyn AsyncRead + Unpin + 'static>,
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

	/// Creates an async body from a standard file.
	#[cfg(feature = "asyncfs")]
	pub fn from_file_async(file: std::fs::File) -> Self {
		let size = file.metadata().map(|meta| meta.len()).ok();
		Body::Async {
			data: Box::new(async_fs::File::from(file)),
			len: size,
		}
	}

	/// Creates a body from an async file.
	#[cfg(feature = "asyncfs")]
	pub async fn from_async_file(file: async_fs::File) -> Self {
		let size = file.metadata().await.map(|meta| meta.len()).ok();
		Body::Async {
			data: Box::new(file),
			len: size,
		}
	}

	/// Returns the length of the body, if known.
	pub fn len(&self) -> Option<u64> {
		match self {
			Body::Immediate(data) => Some(data.len() as u64),
			Body::Sync { data: _, len } => *len,
			Body::Async { data: _, len } => *len,
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
		let size = file.metadata().map(|meta| meta.len()).ok();
		Body::Sync {
			data: Box::new(file),
			len: size,
		}
	}
}

#[cfg(feature = "asyncfs")]
impl From<async_fs::File> for Body {
	fn from(file: async_fs::File) -> Self {
		Body::Async {
			data: Box::new(file),
			len: None,
		}
	}
}

impl Debug for Body {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Immediate(v) => write!(f, "Body(Immediate, len={})", v.len()),
			Self::Sync { data: _, len } => write!(f, "Body(Sync, len={len:?})"),
			Self::Async { data: _, len } => write!(f, "Body(Async, len={len:?})"),
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
}
