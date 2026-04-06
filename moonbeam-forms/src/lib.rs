#![doc = include_str!("../README.md")]

use moonbeam::{
	Body,
	http::{
		FromRequest, Request, Response,
		cookies::Cookies,
		params::{AllParamIter, ParamIter, Params},
	},
};

/// Represents a single piece of form data.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum FormData<'a> {
	/// Simple text data.
	Text(&'a str),
	/// File upload data.
	File {
		/// The original filename provided by the client, if any.
		name: Option<&'a str>,
		/// The content type of the file, if any.
		content_type: Option<&'a str>,
		/// The raw bytes of the file.
		data: &'a [u8],
	},
}

/// An extractor for HTML form data.
///
/// Handles both URL-encoded and multipart form data depending on the request's
/// `Content-Type`.
#[non_exhaustive]
pub enum Form<'a> {
	/// URL-encoded form data.
	URLEncoded(Params<'a>),
	/// Multipart form data.
	Multipart(Multipart<'a>),
}

impl<'a> Form<'a> {
	/// Returns an iterator over values for a specific field name.
	pub fn find<'b>(&self, name: &'b str) -> FormIterator<'a, 'b, '_> {
		match self {
			Form::URLEncoded(p) => FormIterator::URLEncoded(p.find(name)),
			Form::Multipart(m) => FormIterator::Multipart(m.find(name)),
		}
	}

	/// Returns an iterator over all form fields.
	pub fn iter(&self) -> AllFormIterator<'a, '_> {
		match self {
			Form::URLEncoded(p) => AllFormIterator::URLEncoded(p.iter()),
			Form::Multipart(m) => AllFormIterator::Multipart(m.iter()),
		}
	}
}

impl<'a> TryFrom<Request<'_, 'a>> for Form<'a> {
	type Error = FormError;

	fn try_from(req: Request<'_, 'a>) -> Result<Self, Self::Error> {
		if req.method.eq_ignore_ascii_case("get") {
			return Ok(Form::URLEncoded(req.params()));
		} else if !req.method.eq_ignore_ascii_case("post") {
			return Err(FormError::MethodNotAllowed);
		}

		let content_type = req
			.find_header("Content-Type")
			.ok_or(FormError::MissingOrInvalidContentType)?;

		if content_type.starts_with(b"application/x-www-form-urlencoded") {
			// Ensure valid UTF-8
			let body = std::str::from_utf8(req.body).map_err(|_| FormError::InvalidUtf8)?;
			Ok(Form::URLEncoded(Params::new(body)))
		} else if content_type.starts_with(b"multipart/form-data") {
			let search = Cookies::new(Some(content_type));
			let boundary = search
				.find("boundary")
				.ok_or(FormError::MissingMutlipartBoundary)?;

			Ok(Form::Multipart(Multipart::new(boundary, req.body)))
		} else {
			Err(FormError::MissingOrInvalidContentType)
		}
	}
}

impl<'a, S> FromRequest<'_, 'a, S> for Form<'a> {
	type Error = FormError;

	async fn from_request(req: Request<'_, 'a>, _state: &'static S) -> Result<Self, Self::Error> {
		Self::try_from(req)
	}
}

/// An iterator over form fields.
#[non_exhaustive]
pub enum FormIterator<'a, 'b, 'c> {
	/// Iterator over specific URL-encoded form fields.
	URLEncoded(ParamIter<'c, 'b>),
	/// Iterator over multipart form fields.
	Multipart(PartIter<'a, 'b, 'c>),
}

impl<'a> Iterator for FormIterator<'_, '_, 'a> {
	type Item = FormData<'a>;

	fn next(&mut self) -> Option<Self::Item> {
		match self {
			FormIterator::URLEncoded(p) => p.next().map(FormData::Text),
			FormIterator::Multipart(p) => p.next(),
		}
	}
}

/// An iterator over all form fields.
#[non_exhaustive]
pub enum AllFormIterator<'a, 'b> {
	/// Iterator over all URL-encoded form fields.
	URLEncoded(AllParamIter<'b>),
	/// Iterator over all multipart form fields.
	Multipart(AllPartIter<'a, 'b>),
}

impl<'a, 'b> Iterator for AllFormIterator<'a, 'b> {
	type Item = (Option<&'b str>, FormData<'b>);

	fn next(&mut self) -> Option<Self::Item> {
		match self {
			AllFormIterator::URLEncoded(p) => p.next().map(|(k, v)| (Some(k), FormData::Text(v))),
			AllFormIterator::Multipart(p) => p.next(),
		}
	}
}

/// Errors that can occur during form extraction.
#[derive(Debug)]
#[non_exhaustive]
pub enum FormError {
	/// The request body was not valid UTF-8 for a URL-encoded form.
	InvalidUtf8,
	/// The `Content-Type` header was missing or invalid for form data.
	MissingOrInvalidContentType,
	/// The request method was not POST (or GET for query-based forms).
	MethodNotAllowed,
	/// The `multipart/form-data` request was missing the required `boundary` parameter.
	MissingMutlipartBoundary,
}

impl From<FormError> for Response {
	fn from(err: FormError) -> Self {
		match err {
			FormError::InvalidUtf8 => {
				Response::bad_request().with_body("Invalid UTF-8", Body::TEXT)
			}
			FormError::MissingOrInvalidContentType => Response::bad_request()
				.with_body("Missing or invalid Content-Type header", Body::TEXT),
			FormError::MissingMutlipartBoundary => {
				Response::bad_request().with_body("Missing multipart boundary", Body::TEXT)
			}
			FormError::MethodNotAllowed => Response::method_not_allowed(),
		}
	}
}

/// An extractor for multipart form data (`multipart/form-data`).
pub struct Multipart<'a> {
	parts: Vec<&'a [u8]>,
}

impl<'a> Multipart<'a> {
	/// Creates a new `Multipart` struct from the given boundary and body.
	pub fn new(boundary: &'a [u8], body: &'a [u8]) -> Multipart<'a> {
		let mut parts = vec![];
		let bound = find_next_boundary(body, boundary, b"--");

		if let Some((_, mut end)) = bound {
			while let Some((nstart, nend)) = find_next_boundary(&body[end..], boundary, b"\r\n--") {
				parts.push(&body[end..end + nstart]);
				end += nend;
			}
		}

		Multipart { parts }
	}

	/// Returns an iterator over parts matching a specific field name.
	pub fn find<'b>(&self, name: &'b str) -> PartIter<'a, 'b, '_> {
		PartIter {
			remaining: &self.parts,
			filter: name,
		}
	}

	/// Returns an iterator over all parts in the multipart data.
	pub fn iter(&self) -> AllPartIter<'a, '_> {
		AllPartIter {
			remaining: &self.parts,
		}
	}
}

/// An iterator over parts in a multipart form data request.
pub struct PartIter<'a, 'b, 'c> {
	remaining: &'c [&'a [u8]],
	filter: &'b str,
}

impl<'a> Iterator for PartIter<'a, '_, '_> {
	type Item = FormData<'a>;

	fn next(&mut self) -> Option<Self::Item> {
		while let Some(&part) = self.remaining.split_off_first() {
			let mut headers = [httparse::EMPTY_HEADER; 4];
			let (offset, headers) = match httparse::parse_headers(part, &mut headers) {
				Ok(httparse::Status::Complete(v)) => v,
				_ => return None,
			};
			let h = Cookies::new(
				headers
					.iter()
					.find(|h| h.name.eq_ignore_ascii_case("content-disposition"))
					.map(|h| h.value),
			);
			if h.find("name")
				.filter(|&n| n == self.filter.as_bytes())
				.is_some()
			{
				let filename = h.find("filename");
				let content_type = headers
					.iter()
					.find(|h| h.name.eq_ignore_ascii_case("content-type"))
					.map(|h| h.value);
				if filename.is_some() || content_type.filter(|&v| v != b"text/plan").is_some() {
					return Some(FormData::File {
						name: filename.and_then(|f| std::str::from_utf8(f).ok()),
						content_type: content_type.and_then(|c| std::str::from_utf8(c).ok()),
						data: &part[offset..],
					});
				} else if let Ok(s) = std::str::from_utf8(&part[offset..]) {
					return Some(FormData::Text(s));
				}
			}
		}
		None
	}
}

/// An iterator over parts in a multipart form data request.
pub struct AllPartIter<'a, 'b> {
	remaining: &'b [&'a [u8]],
}

impl<'a> Iterator for AllPartIter<'a, '_> {
	type Item = (Option<&'a str>, FormData<'a>);

	fn next(&mut self) -> Option<Self::Item> {
		while let Some(&part) = self.remaining.split_off_first() {
			let mut headers = [httparse::EMPTY_HEADER; 4];
			let (offset, headers) = match httparse::parse_headers(part, &mut headers) {
				Ok(httparse::Status::Complete(v)) => v,
				_ => return None,
			};
			let h = Cookies::new(
				headers
					.iter()
					.find(|h| h.name.eq_ignore_ascii_case("content-disposition"))
					.map(|h| h.value),
			);
			let name = h.find("name").and_then(|v| str::from_utf8(v).ok());
			let filename = h.find("filename");
			let content_type = headers
				.iter()
				.find(|h| h.name.eq_ignore_ascii_case("content-type"))
				.map(|h| h.value);
			if filename.is_some() || content_type.filter(|&v| v != b"text/plan").is_some() {
				return Some((
					name,
					FormData::File {
						name: filename.and_then(|f| std::str::from_utf8(f).ok()),
						content_type: content_type.and_then(|c| std::str::from_utf8(c).ok()),
						data: &part[offset..],
					},
				));
			} else if let Ok(s) = std::str::from_utf8(&part[offset..]) {
				return Some((name, FormData::Text(s)));
			}
		}
		None
	}
}

fn find_next_boundary(
	body: &[u8],
	boundary: &[u8],
	prefix: &'static [u8],
) -> Option<(usize, usize)> {
	body.windows(boundary.len() + prefix.len())
		.position(|window| {
			&window[0..prefix.len()] == prefix && &window[prefix.len()..] == boundary
		})
		.map(|p| (p, p + prefix.len() + boundary.len() + 2)) // prefixboundary\r\n
}

#[cfg(test)]
mod tests {
	use super::*;
	use moonbeam::Header;

	#[test]
	fn test_get_urlencoded_form() {
		let request = Request::new("GET", "/test?foo=bar&baz=qux&foo=asd", &[], &[]);
		let form = Form::try_from(request).unwrap();
		let mut it = form.find("foo");
		assert_eq!(it.next(), Some(FormData::Text("bar")));
		assert_eq!(it.next(), Some(FormData::Text("asd")));
		assert_eq!(it.next(), None);
		it = form.find("baz");
		assert_eq!(it.next(), Some(FormData::Text("qux")));
		assert_eq!(it.next(), None);

		let mut it = form.iter();
		assert_eq!(it.next(), Some((Some("foo"), FormData::Text("bar"))));
		assert_eq!(it.next(), Some((Some("baz"), FormData::Text("qux"))));
		assert_eq!(it.next(), Some((Some("foo"), FormData::Text("asd"))));
		assert_eq!(it.next(), None);
	}

	#[test]
	fn test_post_urlencoded_form() {
		let headers = [Header {
			name: "Content-Type",
			value: b"application/x-www-form-urlencoded",
		}];
		let request = Request::new("POST", "/test", &headers, b"foo=bar&baz=qux&foo=asd");
		let form = Form::try_from(request).unwrap();
		let mut it = form.find("foo");
		assert_eq!(it.next(), Some(FormData::Text("bar")));
		assert_eq!(it.next(), Some(FormData::Text("asd")));
		assert_eq!(it.next(), None);
		it = form.find("baz");
		assert_eq!(it.next(), Some(FormData::Text("qux")));
		assert_eq!(it.next(), None);

		let mut it = form.iter();
		assert_eq!(it.next(), Some((Some("foo"), FormData::Text("bar"))));
		assert_eq!(it.next(), Some((Some("baz"), FormData::Text("qux"))));
		assert_eq!(it.next(), Some((Some("foo"), FormData::Text("asd"))));
		assert_eq!(it.next(), None);
	}

	#[test]
	fn test_basic_multipart_form() {
		let headers = [Header {
			name: "Content-Type",
			value: b"multipart/form-data; boundary=WebKitFormBoundary",
		}];
		let body = b"--WebKitFormBoundary\r\n\
					Content-Disposition: form-data; name=\"foo\"\r\n\
					\r\n\
					bar\r\n\
					--WebKitFormBoundary\r\n\
					Content-Disposition: form-data; name=\"baz\"\r\n\
					\r\n\
					qux\r\n\
					--WebKitFormBoundary\r\n\
					Content-Disposition: form-data; name=\"foo\"\r\n\
					\r\n\
					asd\r\n\
					--WebKitFormBoundary--";
		let request = Request::new("POST", "/test", &headers, body);
		let form = Form::try_from(request).unwrap();
		let mut it = form.find("foo");
		assert_eq!(it.next(), Some(FormData::Text("bar")));
		assert_eq!(it.next(), Some(FormData::Text("asd")));
		assert_eq!(it.next(), None);
		it = form.find("baz");
		assert_eq!(it.next(), Some(FormData::Text("qux")));
		assert_eq!(it.next(), None);

		let mut it = form.iter();
		assert_eq!(it.next(), Some((Some("foo"), FormData::Text("bar"))));
		assert_eq!(it.next(), Some((Some("baz"), FormData::Text("qux"))));
		assert_eq!(it.next(), Some((Some("foo"), FormData::Text("asd"))));
		assert_eq!(it.next(), None);
	}

	#[test]
	fn test_multipart_form_with_file() {
		let headers = [Header {
			name: "Content-Type",
			value: b"multipart/form-data; boundary=WebKitFormBoundary",
		}];
		let body = b"--WebKitFormBoundary\r\n\
					Content-Disposition: form-data; name=\"foo\"\r\n\
					\r\n\
					bar\r\n\
					--WebKitFormBoundary\r\n\
					Content-Disposition: form-data; name=\"baz\"; filename=\"test\"\r\n\
					\r\n\
					qux\r\n\
					--WebKitFormBoundary\r\n\
					Content-Disposition: form-data; name=\"asd\"; filename=\"\"\r\n\
					Content-Type: application/json\r\n\
					\r\n\
					{\"hello\": \"world\"}\r\n\
					--WebKitFormBoundary--";
		let request = Request::new("POST", "/test", &headers, body);
		let form = Form::try_from(request).unwrap();
		let mut it = form.find("foo");
		assert_eq!(it.next(), Some(FormData::Text("bar")));
		assert_eq!(it.next(), None);
		it = form.find("baz");
		assert_eq!(
			it.next(),
			Some(FormData::File {
				name: Some("test"),
				content_type: None,
				data: b"qux",
			})
		);
		assert_eq!(it.next(), None);
		it = form.find("asd");
		assert_eq!(
			it.next(),
			Some(FormData::File {
				name: Some(""),
				content_type: Some("application/json"),
				data: b"{\"hello\": \"world\"}",
			})
		);
		assert_eq!(it.next(), None);
	}
}
