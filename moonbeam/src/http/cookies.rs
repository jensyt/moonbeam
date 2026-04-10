//! # Cookies Module
//!
//! This module provides the `Cookies` helper for parsing and extracting cookies from the HTTP
//! `Cookie` header.
//!
//! ## Extraction
//!
//! Cookies are typically accessed via the `cookies()` method on the `Request` object.
//!
//! ### Example
//! ```rust
//! use moonbeam::Request;
//!
//! fn handle(req: Request) {
//!     let cookies = req.cookies();
//!     let user_id = cookies.find("user_id");
//!     // ...
//! }
//! ```

/// Helper struct for parsing cookies from a request.
///
/// # Example
/// ```
/// use moonbeam::http::cookies::Cookies;
///
/// let cookie_header = b"user=alice; session=123";
/// let cookies = Cookies::new(Some(cookie_header));
///
/// assert_eq!(cookies.find("user"), Some(b"alice" as &[u8]));
/// assert_eq!(cookies.find("session"), Some(b"123" as &[u8]));
/// ```
pub struct Cookies<'a> {
	cookies: &'a [u8],
}

impl<'a> Cookies<'a> {
	/// Creates a new `Cookies` helper from the Cookie header value.
	pub fn new(cookies: Option<&'a [u8]>) -> Self {
		Cookies {
			cookies: cookies.unwrap_or(b""),
		}
	}

	/// Finds the value of a specific cookie by name.
	///
	/// If the cookie value is enclosed in double quotes, they are stripped from the returned value.
	///
	/// # Example
	/// ```
	/// use moonbeam::http::cookies::Cookies;
	///
	/// let cookies = Cookies::new(Some(b"user=\"alice\""));
	/// assert_eq!(cookies.find("user"), Some(b"alice" as &[u8]));
	/// ```
	pub fn find(&self, cookie: &str) -> Option<&'a [u8]> {
		for mut c in self.cookies.split(|&v| v == b';') {
			// Strip all leading whitespace
			while let Some((b' ', rest)) = c.split_first() {
				c = rest;
			}
			let mut split = c.splitn(2, |&v| v == b'=');
			let name = split.next()?;
			let mut value = match split.next() {
				Some(v) => v,
				// Allow simple malformed cookies (name, no value)
				None => continue,
			};

			if name == cookie.as_bytes() {
				// Handle quoted values correctly
				if value.len() >= 2 && value.first() == Some(&b'"') && value.last() == Some(&b'"') {
					value = &value[1..value.len() - 1];
				}
				return Some(value);
			}
		}
		None
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_find_cookie() {
		let cookies = Cookies::new(Some(b"foo=bar; baz=qux; test=\"quotes\"; qux"));
		assert_eq!(cookies.find("foo"), Some(b"bar" as &[u8]));
		assert_eq!(cookies.find("baz"), Some(b"qux" as &[u8]));
		assert_eq!(cookies.find("test"), Some(b"quotes" as &[u8]));
		assert_eq!(cookies.find("qux"), None);

		// Test multiple spaces
		let cookies = Cookies::new(Some(b"foo=bar;  baz=qux"));
		assert_eq!(cookies.find("baz"), Some(b"qux" as &[u8]));

		// Test malformed quotes
		let cookies = Cookies::new(Some(b"foo=\"bar"));
		assert_eq!(cookies.find("foo"), Some(b"\"bar" as &[u8]));

		let cookies = Cookies::new(Some(b"foo=bar\""));
		assert_eq!(cookies.find("foo"), Some(b"bar\"" as &[u8]));

		let cookies = Cookies::new(Some(b"foo=\"\""));
		assert_eq!(cookies.find("foo"), Some(b"" as &[u8]));

		let cookies = Cookies::new(Some(b"foo; baz=qux"));
		assert_eq!(cookies.find("foo"), None);
		assert_eq!(cookies.find("baz"), Some(b"qux" as &[u8]));
	}
}
