//! # Path Parameters Module
//!
//! This module provides the `PathParams` extractor for the Moonbeam router.
//!
//! ## How it Works
//!
//! When you define a route with parameters (e.g., `/users/:id`), the router extracts
//! the matching segments and provides them as a list of strings. The `#[route]`
//! macro then uses the `FromParams` trait to convert this list into a strongly-typed
//! `PathParams` object.
//!
//! ### Common Patterns
//!
//! - **Single Parameter**: `/users/:id` -> `PathParams<&str>`
//! - **Multiple Parameters**: `/users/:id/posts/:post_id` -> `PathParams<(&str, &str)>`
//! - **Rest Parameters**: `/static/*path` -> `PathParams<&str>`
//!
//! ## Safety
//!
//! Path parameters are extracted from the URL and provided to your handler as borrowed
//! string slices. These slices are valid for the duration of the handler's execution.

/// Wrapper for accessing path parameters extracted by the router.
///
/// This struct is used with the `#[route]` macro to extract named parameters from the URL path.
/// It primarily supports tuple destructuring to map path segments to function arguments.
///
/// # Examples
///
/// ## Single Parameter
/// ```rust,no_run
/// use moonbeam::{route, Response, Body};
/// use moonbeam::router::PathParams;
///
/// // Handler signature for path "/users/:id"
/// #[route]
/// async fn get_user(PathParams(id): PathParams<&str>) -> Response {
///     Response::ok().with_body(format!("User ID: {}", id), Body::TEXT)
/// }
/// ```
///
/// ## Multiple Parameters (Tuple Destructuring)
/// ```rust,no_run
/// use moonbeam::{route, Response, Body};
/// use moonbeam::router::PathParams;
///
/// // Handler signature for path "/users/:id/posts/:post_id"
/// #[route]
/// async fn get_post(PathParams((user_id, post_id)): PathParams<(&str, &str)>) -> Response {
///     Response::ok().with_body(format!("User: {}, Post: {}", user_id, post_id), Body::TEXT)
/// }
/// ```
///
/// ## Rest Parameters
/// ```rust,no_run
/// use moonbeam::{route, Response, Body};
/// use moonbeam::router::PathParams;
///
/// // Handler signature for path "/static/*path"
/// #[route]
/// async fn serve_static(PathParams(path): PathParams<&str>) -> Response {
///     // For path "/static/css/style.css", id will be "css/style.css"
///     Response::ok().with_body(format!("Path: {}", path), Body::TEXT)
/// }
/// ```
///
/// Note that rest parameters match zero or more path segments, so this example will match "/static",
/// "/static/", "/static/a", "/static/a/b", etc.
#[derive(Debug)]
#[cfg_attr(docsrs, doc(cfg(feature = "router")))]
pub struct PathParams<T>(pub T);

/// Trait for converting a raw parameter list into the target `PathParams` type.
///
/// This allows the `#[route]` macro to automatically convert the slice of parameter values
/// provided by the router into the specific tuple requested by the user.
///
/// # Implementation
///
/// The trait is implemented for `PathParams<T>` where `T` is `&str` or a tuple of `&str`
/// (up to 5 elements).
#[cfg_attr(docsrs, doc(cfg(feature = "router")))]
pub trait FromParams<'a> {
	/// Converts a slice of path parameter strings into `Self`.
	fn from_params(params: &[&'a str]) -> Self;
}

impl<'a> FromParams<'a> for PathParams<&'a str> {
	fn from_params(params: &[&'a str]) -> Self {
		let p1 = params.first().copied().unwrap_or_default();
		PathParams(p1)
	}
}

impl<'a> FromParams<'a> for PathParams<(&'a str,)> {
	fn from_params(params: &[&'a str]) -> Self {
		let p1 = params.first().copied().unwrap_or_default();
		PathParams((p1,))
	}
}

impl<'a> FromParams<'a> for PathParams<(&'a str, &'a str)> {
	fn from_params(params: &[&'a str]) -> Self {
		let p1 = params.first().copied().unwrap_or_default();
		let p2 = params.get(1).copied().unwrap_or_default();
		PathParams((p1, p2))
	}
}

impl<'a> FromParams<'a> for PathParams<(&'a str, &'a str, &'a str)> {
	fn from_params(params: &[&'a str]) -> Self {
		let p1 = params.first().copied().unwrap_or_default();
		let p2 = params.get(1).copied().unwrap_or_default();
		let p3 = params.get(2).copied().unwrap_or_default();
		PathParams((p1, p2, p3))
	}
}

impl<'a> FromParams<'a> for PathParams<(&'a str, &'a str, &'a str, &'a str)> {
	fn from_params(params: &[&'a str]) -> Self {
		let p1 = params.first().copied().unwrap_or_default();
		let p2 = params.get(1).copied().unwrap_or_default();
		let p3 = params.get(2).copied().unwrap_or_default();
		let p4 = params.get(3).copied().unwrap_or_default();
		PathParams((p1, p2, p3, p4))
	}
}

impl<'a> FromParams<'a> for PathParams<(&'a str, &'a str, &'a str, &'a str, &'a str)> {
	fn from_params(params: &[&'a str]) -> Self {
		let p1 = params.first().copied().unwrap_or_default();
		let p2 = params.get(1).copied().unwrap_or_default();
		let p3 = params.get(2).copied().unwrap_or_default();
		let p4 = params.get(3).copied().unwrap_or_default();
		let p5 = params.get(4).copied().unwrap_or_default();
		PathParams((p1, p2, p3, p4, p5))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_from_params_tuple_str() {
		let input = ["v1", "v2"];
		let params: PathParams<(&str, &str)> = FromParams::from_params(&input);
		assert_eq!(params.0.0, "v1");
		assert_eq!(params.0.1, "v2");
	}
}
