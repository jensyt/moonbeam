//! Utility for parsing and iterating over HTTP request paths.
//!
//! This module provides the `PathIterator` helper for parsing and iterating over path segments from
//! the URL.
//!
//! ## Extraction
//!
//! Path segments can be accessed via the `url()` method on the `Request` object. Path segments are
//! automatically URL-decoded.
//!
//! ### Example
//! ```rust
//! use moonbeam::Request;
//!
//! fn handle(req: Request) {
//!     let url: Vec<_> = req.url().collect();
//!     // ...
//! }
//! ```

use super::percent_decode::PercentDecode;
use std::borrow::Cow;

/// A zero-copy iterator over percent-decoded URL path segments.
///
/// This iterator yields each segment of the path, including the leading `/`.
/// For example, `/path/to/somewhere` yields `/path`, `/to`, and `/somewhere`.
pub struct PathIterator<'buf> {
	remainder: &'buf str,
}

impl<'buf> PathIterator<'buf> {
	/// Creates a new `PathIterator` from the given raw path string.
	pub fn new(input: &'buf str) -> Self {
		Self { remainder: input }
	}
}

impl<'buf> Iterator for PathIterator<'buf> {
	type Item = Cow<'buf, str>;

	fn next(&mut self) -> Option<Self::Item> {
		if self.remainder.is_empty() {
			return None;
		}

		// We use char_indices to find the next delimiter, skipping the first character to avoid an
		// empty first match.
		let mut chars = self.remainder.char_indices();
		chars.next();

		let index = match chars.find(|&(_, c)| c == '/') {
			Some((index, _)) => index,
			None => self.remainder.len(),
		};
		let (part, rest) = self.remainder.split_at(index);
		self.remainder = rest;
		Some(part.percent_decode())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_path_iterator() {
		let cases = vec![
			("/", vec!["/"]),
			("//", vec!["/", "/"]),
			("/path", vec!["/path"]),
			("path", vec!["path"]),
			("/path/to/somewhere", vec!["/path", "/to", "/somewhere"]),
			("", vec![]),
			("/path/", vec!["/path", "/"]),
			("/path//", vec!["/path", "/", "/"]),
			("/path//to", vec!["/path", "/", "/to"]),
			("/path/./to", vec!["/path", "/.", "/to"]),
			("/path/../to", vec!["/path", "/..", "/to"]),
			("/path%20with%20space", vec!["/path with space"]),
		];

		for (input, expected) in cases {
			let actual: Vec<_> = PathIterator::new(input).collect();
			assert_eq!(actual, expected, "Failed on input: {}", input);
		}
	}
}
