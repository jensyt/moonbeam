/// Wrapper for accessing path parameters extracted by the router.
///
/// This struct is used with the `#[route]` macro to extract named parameters from the URL path.
/// It primarily supports tuple destructuring to map path segments to function arguments.
///
/// # Examples
///
/// ## Single Parameter
/// ```
/// use moonbeam::router::PathParams;
///
/// // Handler signature for path "/users/:id"
/// // fn handler(PathParams(id): PathParams<&str>, ...)
/// ```
///
/// ## Multiple Parameters (Tuple Destructuring)
/// ```
/// use moonbeam::router::PathParams;
///
/// // Handler signature for path "/users/:id/posts/:post_id"
/// // fn handler(PathParams((id, post_id)): PathParams<(&str, &str)>, ...)
/// ```
///
/// ## Rest Parameters
/// ```ignore
/// use moonbeam::router::PathParams;
///
/// // Handler signature for path "/static/*path"
/// fn handler(PathParams(path): PathParams<&str>, ...)
/// ```
///
/// Note that rest parameters match zero or more path segments, so this example will match "/static",
/// "/static/", "/static/a", "/static/a/b", etc.
#[derive(Debug)]
pub struct PathParams<T>(pub T);

/// Trait for converting a raw parameter list into the target `PathParams` type.
///
/// This allows the `#[route]` macro to automatically convert the slice of parameter values
/// provided by the router into the specific tuple or structure requested by the user.
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
