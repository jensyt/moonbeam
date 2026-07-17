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

use super::percent_decode::PercentDecode;
use std::{borrow::Cow, cmp::Ordering};

/// Helper struct for parsing query parameters from a URL.
///
/// # Example
/// ```
/// use std::borrow::Cow;
/// use moonbeam::http::params::Params;
///
/// let params = Params::new("key=value");
/// assert_eq!(params.find("key").next(), Some(Cow::Borrowed("value")));
/// ```
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Params<'buf> {
	params: &'buf str,
}

impl<'buf> Params<'buf> {
	/// Creates a new `Params` helper from the query string.
	pub fn new(params: &'buf str) -> Self {
		Params { params }
	}

	/// Returns an iterator over values for a specific parameter name.
	///
	/// # Example
	/// ```
	/// use std::borrow::Cow;
	/// use moonbeam::http::params::Params;
	///
	/// let params = Params::new("a=1&b=2&a=3");
	/// let mut a = params.find("a");
	/// assert_eq!(a.next(), Some(Cow::Borrowed("1")));
	/// assert_eq!(a.next(), Some(Cow::Borrowed("3")));
	/// assert_eq!(a.next(), None);
	/// ```
	pub fn find<'p>(&self, param: &'p str) -> ParamIter<'buf, 'p> {
		ParamIter::new(self.params, param)
	}

	/// Returns an iterator over all key-value pairs.
	///
	/// # Example
	/// ```
	/// use std::borrow::Cow;
	/// use moonbeam::http::params::Params;
	///
	/// let params = Params::new("a=1&b=2");
	/// let mut it = params.iter();
	/// assert_eq!(it.next(), Some((Cow::Borrowed("a"), Cow::Borrowed("1"))));
	/// assert_eq!(it.next(), Some((Cow::Borrowed("b"), Cow::Borrowed("2"))));
	/// assert_eq!(it.next(), None);
	/// ```
	pub fn iter(&self) -> AllParamIter<'buf> {
		AllParamIter {
			remaining: self.params,
		}
	}
}

/// Iterator over all query parameters as key-value pairs.
#[derive(Debug, Clone, Copy, Eq)]
pub struct AllParamIter<'buf> {
	remaining: &'buf str,
}

impl<'buf> Iterator for AllParamIter<'buf> {
	type Item = (Cow<'buf, str>, Cow<'buf, str>);

	fn next(&mut self) -> Option<Self::Item> {
		if self.remaining.is_empty() {
			return None;
		}

		let (current_param, rest) = self
			.remaining
			.split_once('&')
			.unwrap_or((self.remaining, ""));

		self.remaining = rest;

		if let Some((k, v)) = current_param.split_once('=') {
			Some((k.percent_decode_query(), v.percent_decode_query()))
		} else {
			Some((current_param.percent_decode_query(), Cow::Borrowed("")))
		}
	}
}

impl<'buf> AllParamIter<'buf> {
	/// Creates a new `AllParamIter` to iterate all parameters.
	pub fn new(params: &'buf str) -> Self {
		Self { remaining: params }
	}
}

impl PartialEq for AllParamIter<'_> {
	fn eq(&self, other: &Self) -> bool {
		// Compare by pointer rather than by value
		self.remaining.as_ptr() == other.remaining.as_ptr()
	}
}

impl PartialOrd for AllParamIter<'_> {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		let this = self.remaining.as_bytes().as_ptr_range();
		let other = other.remaining.as_bytes().as_ptr_range();

		if this.contains(&other.start) && other.end == this.end {
			this.start.partial_cmp(&other.start)
		} else {
			None
		}
	}
}

/// Iterator over values for a specific query parameter.
#[derive(Debug, Clone, Copy, Eq)]
pub struct ParamIter<'buf, 'param> {
	remaining: &'buf str,
	filter: &'param str,
}

impl<'buf, 'param> Iterator for ParamIter<'buf, 'param> {
	type Item = Cow<'buf, str>;

	fn next(&mut self) -> Option<Self::Item> {
		while !self.remaining.is_empty() {
			// Find the next parameter (up to '&' or end of string)
			let (current_param, rest) = self
				.remaining
				.split_once('&')
				.unwrap_or((self.remaining, ""));

			// Update remaining for next iteration
			self.remaining = rest;

			// Split the current parameter into key=value
			if let Some((k, v)) = current_param.split_once('=') {
				if k.percent_decode_query() == self.filter {
					return Some(v.percent_decode_query());
				}
			} else if current_param.percent_decode_query() == self.filter {
				return Some(Cow::Borrowed(""));
			}
		}

		None
	}
}

impl<'buf, 'param> ParamIter<'buf, 'param> {
	/// Creates a new `ParamIter` to filter the given query string by parameter name.
	pub fn new(params: &'buf str, filter: &'param str) -> Self {
		Self {
			remaining: params,
			filter,
		}
	}

	/// Returns the parameter name being filtered for.
	pub fn name(&self) -> &'param str {
		self.filter
	}
}

impl PartialEq for ParamIter<'_, '_> {
	fn eq(&self, other: &Self) -> bool {
		self.remaining.as_ptr() == other.remaining.as_ptr() && self.filter == other.filter
	}
}

impl PartialOrd for ParamIter<'_, '_> {
	fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
		if self.filter != other.filter {
			return None;
		}

		let this = self.remaining.as_bytes().as_ptr_range();
		let other = other.remaining.as_bytes().as_ptr_range();

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
	fn test_find_param() {
		let params = Params::new("foo=bar&baz=qux&foo=baz");
		let mut p = params.find("foo");
		assert_eq!(p.next(), Some(Cow::Borrowed("bar")));
		assert_eq!(p.next(), Some(Cow::Borrowed("baz")));
		assert_eq!(p.next(), None);
		p = params.find("baz");
		assert_eq!(p.next(), Some(Cow::Borrowed("qux")));
		assert_eq!(p.next(), None);
		assert_eq!(params.find("qux").next(), None);
	}

	#[test]
	fn test_all_param() {
		let params = Params::new("foo=bar&baz=qux&foo=baz");
		let mut it = params.iter();
		assert_eq!(
			it.next(),
			Some((Cow::Borrowed("foo"), Cow::Borrowed("bar")))
		);
		assert_eq!(
			it.next(),
			Some((Cow::Borrowed("baz"), Cow::Borrowed("qux")))
		);
		assert_eq!(
			it.next(),
			Some((Cow::Borrowed("foo"), Cow::Borrowed("baz")))
		);
		assert_eq!(it.next(), None);
	}

	#[test]
	fn test_encoded_delimiter() {
		let params = Params::new("a=foo%26b=bar");
		let mut p = params.find("a");
		// The %26 should be decoded as part of the value, NOT split
		assert_eq!(p.next(), Some(Cow::Owned::<str>("foo&b=bar".to_string())));
		assert_eq!(p.next(), None);

		let mut p_b = params.find("b");
		assert_eq!(p_b.next(), None);
	}

	#[test]
	fn test_allparamiter_eq() {
		let left = "foo=bar&baz=qux&foo=baz".to_string();
		let right = left.clone();
		let left = AllParamIter::new(left.as_str());
		let right = AllParamIter::new(right.as_str());

		assert_eq!(left, left.clone());
		assert_ne!(left, right);
	}

	#[test]
	fn test_allparamiter_partial_ord() {
		let left = "foo=bar&baz=qux&foo=baz".to_string();
		let right = left.clone();
		let left = AllParamIter::new(left.as_str());
		let right = AllParamIter::new(right.as_str());

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

	#[test]
	fn test_paramiter_eq() {
		let left = "foo=bar&baz=qux&foo=baz".to_string();
		let right = left.clone();
		let left = ParamIter::new(left.as_str(), "foo");
		let right = ParamIter::new(right.as_str(), "foo");

		assert_eq!(left, left.clone());
		assert_ne!(left, right);
	}

	#[test]
	fn test_paramiter_partial_ord() {
		let left = "foo=bar&baz=qux&foo=baz".to_string();
		let right = left.clone();
		let left = ParamIter::new(left.as_str(), "foo");
		let right = ParamIter::new(right.as_str(), "foo");

		let mut clone = left.clone();
		assert!(left <= clone);
		assert!(left >= clone);
		// Note the use of PartialOrd trait - Iterator has a partial_cmp function that compares the
		// values of the two iterators, which in this case will match even though the iterators
		// themselves are not equal
		assert!(PartialOrd::partial_cmp(&left, &right).is_none());
		assert_eq!(left >= right, false);
		assert_eq!(left <= right, false);

		let mut right = left.clone();
		right.filter = "baz";
		assert!(PartialOrd::partial_cmp(&left, &right).is_none());
		assert_eq!(left >= right, false);
		assert_eq!(left <= right, false);

		clone.next();
		assert!(left < clone);
		assert!(!(left >= clone));
	}
}
