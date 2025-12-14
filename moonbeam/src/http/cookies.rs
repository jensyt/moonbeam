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
			match c.split_first() {
				Some((b' ', rest)) => c = rest,
				_ => {}
			}
			let mut split = c.split(|&v| v == b'=');
			match split.next() {
				Some(n) if n == cookie.as_bytes() => {
					if split.next().is_none() {
						continue;
					}
					let v = &c[n.len() + 1..];
					match v.split_first() {
						Some((b'"', rest)) => match rest.split_last() {
							Some((_, rest)) => return Some(rest),
							None => continue,
						},
						_ => return Some(v),
					}
				}
				_ => (),
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
	}
}
