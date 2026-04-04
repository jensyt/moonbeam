//! Extractors for request data.
//!
//! Implements [`FromRequest`] for [`Params`] and [`Cookies`]. Both of these are infallible
//! conversions, simply wrapping the underlying [`Request`] methods.

use crate::http::{FromRequest, Request, cookies::Cookies, params::Params};
use std::convert::Infallible;

impl<'a, 'b, S> FromRequest<'a, 'b, S> for Params<'b> {
	type Error = Infallible;

	async fn from_request(req: Request<'a, 'b>, _state: &'static S) -> Result<Self, Self::Error> {
		Ok(req.params())
	}
}

impl<'a, 'b, S> FromRequest<'a, 'b, S> for Cookies<'a> {
	type Error = Infallible;

	async fn from_request(req: Request<'a, 'b>, _state: &'static S) -> Result<Self, Self::Error> {
		Ok(req.cookies())
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::Header;

	#[test]
	fn test_params_from_request() {
		let request = Request::new("GET", "/test?foo=bar&baz=qux", &[], &[]);
		let params = futures_lite::future::block_on(Params::from_request(request, &())).unwrap();
		assert_eq!(params.find("foo").next(), Some("bar"));
		assert_eq!(params.find("baz").next(), Some("qux"));
	}

	#[test]
	fn test_cookies_from_request() {
		let headers = [Header {
			name: "Cookie",
			value: b"foo=bar; baz=qux",
		}];
		let request = Request::new("GET", "/test", &headers, &[]);
		let cookies = futures_lite::future::block_on(Cookies::from_request(request, &())).unwrap();
		assert_eq!(cookies.find("foo"), Some(b"bar".as_slice()));
		assert_eq!(cookies.find("baz"), Some(b"qux".as_slice()));
	}
}
