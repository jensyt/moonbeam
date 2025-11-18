use std::borrow::Cow;

pub struct Params<'a> {
	params: Cow<'a, str>,
}

impl<'a> Params<'a> {
	pub fn new(params: Cow<'a, str>) -> Self {
		Params { params }
	}

	pub fn find<'b>(&'a self, param: &'b str) -> ParamIter<'a, 'b> {
		ParamIter::new(&self.params, param)
	}
}

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
