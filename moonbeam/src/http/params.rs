//! # Query Parameters Module
//!
//! This module provides the `Params` helper for parsing and extracting query string parameters
//! from the URL.
//!
//! ## Extraction
//!
//! Parameters are typically accessed via the `params()` method on the `Request` object.
//! Query parameters are automatically URL-decoded, including converting '+' to space.
//!
//! ### Example
//! ```rust
//! use moonbeam::Request;
//!
//! fn handle(req: Request) {
//!     let params = req.params();
//!     let q = params.find("q").next();
//!     // ...
//! }
//! ```

use std::borrow::Cow;

/// Helper struct for parsing query parameters from a URL.
///
/// # Example
/// ```
/// use std::borrow::Cow;
/// use moonbeam::http::params::Params;
///
/// let params = Params::new(Cow::Borrowed("key=value"));
/// assert_eq!(params.find("key").next(), Some("value"));
/// ```
pub struct Params<'a> {
	params: Cow<'a, str>,
}

impl<'a> Params<'a> {
	/// Creates a new `Params` helper from the query string.
	pub fn new(params: Cow<'a, str>) -> Self {
		Params { params }
	}

	/// Returns an iterator over values for a specific parameter name.
	///
	/// # Example
	/// ```
	/// use std::borrow::Cow;
	/// use moonbeam::http::params::Params;
	///
	/// let params = Params::new(Cow::Borrowed("a=1&b=2&a=3"));
	/// let mut a = params.find("a");
	/// assert_eq!(a.next(), Some("1"));
	/// assert_eq!(a.next(), Some("3"));
	/// assert_eq!(a.next(), None);
	/// ```
	pub fn find<'b>(&'a self, param: &'b str) -> ParamIter<'a, 'b> {
		ParamIter::new(&self.params, param)
	}
}

/// Iterator over values for a specific query parameter.
pub struct ParamIter<'a, 'b> {
	remaining: &'a str,
	filter: &'b str,
}

impl<'a, 'b> Iterator for ParamIter<'a, 'b> {
	type Item = &'a str;

	fn next(&mut self) -> Option<Self::Item> {
		if self.remaining.is_empty() {
			return None;
		}

		// Find the next parameter (up to '&' or end of string)
		let (current_param, rest) = self
			.remaining
			.split_once('&')
			.unwrap_or((self.remaining, ""));

		// Update remaining for next iteration
		self.remaining = rest;

		// Split the current parameter into key=value
		match current_param.split_once('=') {
			Some((k, v)) if k == self.filter => Some(v),
			_ => self.next(),
		}
	}
}

impl<'a, 'b> ParamIter<'a, 'b> {
	pub fn new(params: &'a str, filter: &'b str) -> Self {
		Self {
			remaining: params,
			filter,
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_find_param() {
		let params = Params::new(Cow::Borrowed("foo=bar&baz=qux&foo=baz"));
		let mut p = params.find("foo");
		assert_eq!(p.next(), Some("bar"));
		assert_eq!(p.next(), Some("baz"));
		assert_eq!(p.next(), None);
		p = params.find("baz");
		assert_eq!(p.next(), Some("qux"));
		assert_eq!(p.next(), None);
		assert_eq!(params.find("qux").next(), None);
	}
}
