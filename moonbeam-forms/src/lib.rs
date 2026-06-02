#![doc = include_str!("../README.md")]

use moonbeam::{
	Body,
	http::{
		FromRequest, Request, Response,
		cookies::Cookies,
		params::{AllParamIter, ParamIter, Params},
	},
};
use std::borrow::Cow;

/// Represents a single piece of form data.
#[derive(Debug, PartialEq, Clone)]
pub enum FormData<'buf> {
	/// Simple text data.
	Text(Cow<'buf, str>),
	/// File upload data.
	File {
		/// The original filename provided by the client, if any.
		name: Option<Cow<'buf, str>>,
		/// The content type of the file, if any.
		content_type: Option<Cow<'buf, str>>,
		/// The raw bytes of the file.
		data: &'buf [u8],
	},
}

/// An extractor for HTML form data.
///
/// Handles both URL-encoded and multipart form data depending on the request's `Content-Type`.
#[non_exhaustive]
pub enum Form<'buf> {
	/// URL-encoded form data.
	URLEncoded(Params<'buf>),
	/// Multipart form data.
	Multipart(Multipart<'buf>),
}

impl<'buf> Form<'buf> {
	/// Returns an iterator over values for a specific field name.
	pub fn find<'p>(&self, param: &'p str) -> FormIterator<'buf, 'p, '_> {
		match self {
			Form::URLEncoded(p) => FormIterator::URLEncoded(p.find(param)),
			Form::Multipart(m) => FormIterator::Multipart(m.find(param)),
		}
	}

	/// Returns an iterator over all form fields.
	pub fn iter(&self) -> AllFormIterator<'buf, '_> {
		match self {
			Form::URLEncoded(p) => AllFormIterator::URLEncoded(p.iter()),
			Form::Multipart(m) => AllFormIterator::Multipart(m.iter()),
		}
	}
}

impl<'buf> TryFrom<Request<'_, 'buf>> for Form<'buf> {
	type Error = FormError;

	fn try_from(req: Request<'_, 'buf>) -> Result<Self, Self::Error> {
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

impl<'buf, S> FromRequest<'_, 'buf, '_, S> for Form<'buf> {
	type Error = FormError;

	async fn from_request(req: Request<'_, 'buf>, _state: &S) -> Result<Self, Self::Error> {
		Self::try_from(req)
	}
}

/// An iterator over form fields.
#[non_exhaustive]
pub enum FormIterator<'buf, 'param, 'parent> {
	/// Iterator over specific URL-encoded form fields.
	URLEncoded(ParamIter<'buf, 'param>),
	/// Iterator over multipart form fields.
	Multipart(PartIter<'buf, 'param, 'parent>),
}

impl<'parent> Iterator for FormIterator<'_, '_, 'parent> {
	type Item = FormData<'parent>;

	fn next(&mut self) -> Option<Self::Item> {
		match self {
			FormIterator::URLEncoded(p) => p.next().map(FormData::Text),
			FormIterator::Multipart(p) => p.next(),
		}
	}
}

/// An iterator over all form fields.
#[non_exhaustive]
pub enum AllFormIterator<'buf, 'parent> {
	/// Iterator over all URL-encoded form fields.
	URLEncoded(AllParamIter<'buf>),
	/// Iterator over all multipart form fields.
	Multipart(AllPartIter<'buf, 'parent>),
}

impl<'buf> Iterator for AllFormIterator<'buf, '_> {
	type Item = (Option<Cow<'buf, str>>, FormData<'buf>);

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
pub struct Multipart<'buf> {
	parts: Vec<&'buf [u8]>,
}

impl<'buf> Multipart<'buf> {
	/// Creates a new `Multipart` struct from the given boundary and body.
	pub fn new(boundary: &'buf [u8], body: &'buf [u8]) -> Multipart<'buf> {
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
	pub fn find<'p, 's>(&'s self, param: &'p str) -> PartIter<'buf, 'p, 's> {
		PartIter {
			remaining: &self.parts,
			filter: param,
		}
	}

	/// Returns an iterator over all parts in the multipart data.
	pub fn iter(&self) -> AllPartIter<'buf, '_> {
		AllPartIter {
			remaining: &self.parts,
		}
	}
}

/// An iterator over parts in a multipart form data request.
pub struct PartIter<'buf, 'param, 'parent> {
	remaining: &'parent [&'buf [u8]],
	filter: &'param str,
}

impl<'buf> Iterator for PartIter<'buf, '_, '_> {
	type Item = FormData<'buf>;

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
						name: filename.map(|f| String::from_utf8_lossy(f)),
						content_type: content_type.map(|c| String::from_utf8_lossy(c)),
						data: &part[offset..],
					});
				} else {
					return Some(FormData::Text(String::from_utf8_lossy(&part[offset..])));
				}
			}
		}
		None
	}
}

/// An iterator over parts in a multipart form data request.
pub struct AllPartIter<'buf, 'parent> {
	remaining: &'parent [&'buf [u8]],
}

impl<'a> Iterator for AllPartIter<'a, '_> {
	type Item = (Option<Cow<'a, str>>, FormData<'a>);

	fn next(&mut self) -> Option<Self::Item> {
		if let Some(&part) = self.remaining.split_off_first() {
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
			let name = h.find("name").map(|v| String::from_utf8_lossy(v));
			let filename = h.find("filename");
			let content_type = headers
				.iter()
				.find(|h| h.name.eq_ignore_ascii_case("content-type"))
				.map(|h| h.value);
			if filename.is_some() || content_type.filter(|&v| v != b"text/plan").is_some() {
				return Some((
					name,
					FormData::File {
						name: filename.map(|f| String::from_utf8_lossy(f)),
						content_type: content_type.map(|c| String::from_utf8_lossy(c)),
						data: &part[offset..],
					},
				));
			} else {
				return Some((
					name,
					FormData::Text(String::from_utf8_lossy(&part[offset..])),
				));
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
		assert_eq!(it.next(), Some(FormData::Text(Cow::Borrowed("bar"))));
		assert_eq!(it.next(), Some(FormData::Text(Cow::Borrowed("asd"))));
		assert_eq!(it.next(), None);
		it = form.find("baz");
		assert_eq!(it.next(), Some(FormData::Text(Cow::Borrowed("qux"))));
		assert_eq!(it.next(), None);

		let mut it = form.iter();
		assert_eq!(
			it.next(),
			Some((
				Some(Cow::Borrowed("foo")),
				FormData::Text(Cow::Borrowed("bar"))
			))
		);
		assert_eq!(
			it.next(),
			Some((
				Some(Cow::Borrowed("baz")),
				FormData::Text(Cow::Borrowed("qux"))
			))
		);
		assert_eq!(
			it.next(),
			Some((
				Some(Cow::Borrowed("foo")),
				FormData::Text(Cow::Borrowed("asd"))
			))
		);
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
		assert_eq!(it.next(), Some(FormData::Text(Cow::Borrowed("bar"))));
		assert_eq!(it.next(), Some(FormData::Text(Cow::Borrowed("asd"))));
		assert_eq!(it.next(), None);
		it = form.find("baz");
		assert_eq!(it.next(), Some(FormData::Text(Cow::Borrowed("qux"))));
		assert_eq!(it.next(), None);

		let mut it = form.iter();
		assert_eq!(
			it.next(),
			Some((
				Some(Cow::Borrowed("foo")),
				FormData::Text(Cow::Borrowed("bar"))
			))
		);
		assert_eq!(
			it.next(),
			Some((
				Some(Cow::Borrowed("baz")),
				FormData::Text(Cow::Borrowed("qux"))
			))
		);
		assert_eq!(
			it.next(),
			Some((
				Some(Cow::Borrowed("foo")),
				FormData::Text(Cow::Borrowed("asd"))
			))
		);
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
		assert_eq!(it.next(), Some(FormData::Text(Cow::Borrowed("bar"))));
		assert_eq!(it.next(), Some(FormData::Text(Cow::Borrowed("asd"))));
		assert_eq!(it.next(), None);
		it = form.find("baz");
		assert_eq!(it.next(), Some(FormData::Text(Cow::Borrowed("qux"))));
		assert_eq!(it.next(), None);

		let mut it = form.iter();
		assert_eq!(
			it.next(),
			Some((
				Some(Cow::Borrowed("foo")),
				FormData::Text(Cow::Borrowed("bar"))
			))
		);
		assert_eq!(
			it.next(),
			Some((
				Some(Cow::Borrowed("baz")),
				FormData::Text(Cow::Borrowed("qux"))
			))
		);
		assert_eq!(
			it.next(),
			Some((
				Some(Cow::Borrowed("foo")),
				FormData::Text(Cow::Borrowed("asd"))
			))
		);
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
		assert_eq!(it.next(), Some(FormData::Text(Cow::Borrowed("bar"))));
		assert_eq!(it.next(), None);
		it = form.find("baz");
		assert_eq!(
			it.next(),
			Some(FormData::File {
				name: Some(Cow::Borrowed("test")),
				content_type: None,
				data: b"qux",
			})
		);
		assert_eq!(it.next(), None);
		it = form.find("asd");
		assert_eq!(
			it.next(),
			Some(FormData::File {
				name: Some(Cow::Borrowed("")),
				content_type: Some(Cow::Borrowed("application/json")),
				data: b"{\"hello\": \"world\"}",
			})
		);
		assert_eq!(it.next(), None);
	}
}
