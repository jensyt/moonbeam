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
use std::{borrow::Cow, cmp::Ordering};

/// A zero-copy iterator over percent-decoded URL path segments.
///
/// This iterator yields each segment of the path, including the leading `/`.
/// For example, `/path/to/somewhere` yields `/path`, `/to`, and `/somewhere`.
#[derive(Debug, Clone, Copy, Eq)]
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

impl PartialEq for PathIterator<'_> {
	fn eq(&self, other: &Self) -> bool {
		// Compare by pointer rather than by value
		self.remainder.as_ptr() == other.remainder.as_ptr()
	}
}

impl PartialOrd for PathIterator<'_> {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		let this = self.remainder.as_bytes().as_ptr_range();
		let other = other.remainder.as_bytes().as_ptr_range();

		if this.contains(&other.start) && other.end == this.end {
			this.start.partial_cmp(&other.start)
		} else {
			None
		}
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

	#[test]
	fn test_eq() {
		let left = "test".to_string();
		let right = left.clone();
		let left = PathIterator::new(left.as_str());
		let right = PathIterator::new(right.as_str());

		assert_eq!(left, left.clone());
		assert_ne!(left, right);
	}

	#[test]
	fn test_partial_ord() {
		let left = "test/path/to/nowhere".to_string();
		let right = left.clone();
		let left = PathIterator::new(left.as_str());
		let right = PathIterator::new(right.as_str());

		let mut clone = left.clone();
		assert!(left <= clone);
		assert!(left >= clone);
		// Note the use of PartialOrd trait - Iterator has a partial_cmp function that compares the
		// values of the two iterators, which in this case will match even though the iterators
		// themselves are not equal
		assert!(PartialOrd::partial_cmp(&left, &right).is_none());
		assert_eq!(left >= right, false);
		assert_eq!(left <= right, false);

		clone.next();
		assert!(left < clone);
		assert!(!(left >= clone));
	}
}
