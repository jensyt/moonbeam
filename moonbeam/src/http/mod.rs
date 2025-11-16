use crate::http::cookies::Cookies;
use futures_lite::AsyncRead;
use httparse::{Header, Request as RawRequest};
use percent_encoding::percent_decode_str;
use std::{
	borrow::{Borrow, Cow},
	io::Read,
};

pub mod cookies;

pub fn canonical_reason(code: u16) -> &'static str {
	match code {
		200 => "OK",
		204 => "No Content",
		304 => "Not Modified",
		400 => "Bad Request",
		401 => "Unauthorized",
		403 => "Forbidden",
		404 => "Not Found",
		500 => "Internal Server Error",
		_ => "",
	}
}

pub struct Request<'headers, 'buf> {
	pub method: &'buf str,
	pub path: &'buf str,
	pub version: u8,
	pub headers: &'headers [Header<'buf>],
	pub body: &'buf [u8],
}

impl<'headers, 'buf> Request<'headers, 'buf> {
	pub fn new(raw: RawRequest<'headers, 'buf>, body: &'buf [u8]) -> Self {
		Self {
			method: raw.method.unwrap(),
			path: raw.path.unwrap(),
			version: raw.version.unwrap(),
			headers: raw.headers,
			body,
		}
	}

	#[inline]
	pub fn find_header(&self, name: &str) -> Option<&[u8]> {
		self.headers
			.iter()
			.find(|h| h.name.eq_ignore_ascii_case(name))
			.map(|h| h.value)
	}

	#[inline]
	pub fn cookies(&self) -> Cookies {
		Cookies::new(self.find_header("Cookies"))
	}

	#[inline]
	pub fn url(&self) -> Cow<'_, str> {
		let url = self.path.split('?').next().unwrap_or(self.path);
		percent_decode_str(url).decode_utf8_lossy()
	}
}

pub struct Response {
	pub status: u16,
	pub headers: Vec<(String, String)>,
	pub body: Option<Body>,
}

impl Response {
	#[inline]
	pub fn new_with_code(status: u16) -> Self {
		Self {
			status,
			headers: Vec::new(),
			body: None,
		}
	}

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

	#[inline]
	pub fn empty() -> Self {
		Self::new_with_code(204)
	}

	#[inline]
	pub fn ok() -> Self {
		Self::new_with_code(200)
	}

	#[inline]
	pub fn not_modified() -> Self {
		Self::new_with_code(304)
	}

	#[inline]
	pub fn not_found() -> Self {
		Self::new_with_code(404)
	}

	#[inline]
	pub fn internal_server_error() -> Self {
		Self::new_with_code(500)
	}

	#[inline]
	pub fn bad_request() -> Self {
		Self::new_with_code(400)
	}

	#[inline]
	pub fn unauthorized() -> Self {
		Self::new_with_code(401)
	}

	#[inline]
	pub fn forbidden() -> Self {
		Self::new_with_code(403)
	}

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

pub enum Body {
	Immediate(Vec<u8>),
	Sync {
		data: Box<dyn Read + 'static>,
		len: Option<u64>,
	},
	Async {
		data: Box<dyn AsyncRead + Unpin + 'static>,
		len: Option<u64>,
	},
}

impl Body {
	pub fn from_vec(data: impl Into<Vec<u8>>) -> Self {
		Self::Immediate(data.into())
	}

	#[cfg(feature = "asyncfs")]
	pub fn from_file_async(file: std::fs::File) -> Self {
		let size = file.metadata().map(|meta| meta.len()).ok();
		Body::Async {
			data: Box::new(async_fs::File::from(file)),
			len: size,
		}
	}

	#[cfg(feature = "asyncfs")]
	pub async fn from_async_file(file: async_fs::File) -> Self {
		let size = file.metadata().await.map(|meta| meta.len()).ok();
		Body::Async {
			data: Box::new(file),
			len: size,
		}
	}

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
