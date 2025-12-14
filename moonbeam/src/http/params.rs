use std::borrow::Cow;

/// Helper struct for parsing query parameters from a URL.
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
	/// For a query string `?a=1&b=2&a=3`, `find("a")` will yield `1` and `3`.
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
