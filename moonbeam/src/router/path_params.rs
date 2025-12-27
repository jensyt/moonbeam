use std::collections::HashMap;

/// Wrapper for accessing path parameters extracted by the router.
///
/// This struct is used with the `#[route]` macro to extract named parameters from the URL path.
/// It is generic to support both direct access to the underlying `HashMap` and
/// tuple destructuring for cleaner signatures.
///
/// # Examples
///
/// ## Accessing via HashMap
/// ```rust
/// use moonbeam::router::PathParams;
/// use std::collections::HashMap;
///
/// // Handler signature
/// // fn handler(PathParams(params): PathParams<HashMap<&str, &str>>, ...)
/// ```
///
/// ## Accessing via Tuple Destructuring
/// ```rust
/// use moonbeam::router::PathParams;
///
/// // Handler signature for path "/users/:id/posts/:post_id"
/// // fn handler(PathParams((id, post_id)): PathParams<(&str, &str)>, ...)
/// ```
#[derive(Debug)]
pub struct PathParams<T>(pub T);

pub type ParamMap<'a> = HashMap<&'a str, &'a str>;
pub type PathParamsMap<'a> = PathParams<ParamMap<'a>>;

impl<'a> From<ParamMap<'a>> for PathParamsMap<'a> {
	fn from(params: ParamMap<'a>) -> Self {
		Self(params)
	}
}

/// Trait for converting a raw parameter map into the target `PathParams` type.
/// This allows the `#[route]` macro to automatically convert the map provided by the router
/// into the specific tuple or structure requested by the user.
pub trait FromParams<'a> {
	fn from_params(params: ParamMap<'a>, names: &[&str]) -> Self;
}

// Default implementation for HashMap<&str, &str>
impl<'a> FromParams<'a> for PathParamsMap<'a> {
	fn from_params(params: ParamMap<'a>, _names: &[&str]) -> Self {
		PathParams(params)
	}
}

impl<'a> FromParams<'a> for PathParams<&'a str> {
	fn from_params(mut params: ParamMap<'a>, names: &[&str]) -> Self {
		let p1 = params.remove(names[0]).unwrap_or_default();
		PathParams(p1)
	}
}

// Implementations for Tuples (up to 4 for now, can be expanded)
impl<'a> FromParams<'a> for PathParams<(&'a str,)> {
	fn from_params(mut params: ParamMap<'a>, names: &[&str]) -> Self {
		let p1 = params.remove(names[0]).unwrap_or_default();
		PathParams((p1,))
	}
}

impl<'a> FromParams<'a> for PathParams<(&'a str, &'a str)> {
	fn from_params(mut params: ParamMap<'a>, names: &[&str]) -> Self {
		let p1 = params.remove(names[0]).unwrap_or_default();
		let p2 = params.remove(names[1]).unwrap_or_default();
		PathParams((p1, p2))
	}
}

impl<'a> FromParams<'a> for PathParams<(&'a str, &'a str, &'a str)> {
	fn from_params(mut params: ParamMap<'a>, names: &[&str]) -> Self {
		let p1 = params.remove(names[0]).unwrap_or_default();
		let p2 = params.remove(names[1]).unwrap_or_default();
		let p3 = params.remove(names[2]).unwrap_or_default();
		PathParams((p1, p2, p3))
	}
}

impl<'a> FromParams<'a> for PathParams<(&'a str, &'a str, &'a str, &'a str)> {
	fn from_params(mut params: ParamMap<'a>, names: &[&str]) -> Self {
		let p1 = params.remove(names[0]).unwrap_or_default();
		let p2 = params.remove(names[1]).unwrap_or_default();
		let p3 = params.remove(names[2]).unwrap_or_default();
		let p4 = params.remove(names[3]).unwrap_or_default();
		PathParams((p1, p2, p3, p4))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_from_params_hashmap_str() {
		let mut input = HashMap::new();
		input.insert("key", "value");
		let params: PathParams<HashMap<&str, &str>> = FromParams::from_params(input, &[]);
		assert_eq!(params.0.get("key"), Some(&"value"));
	}

	#[test]
	fn test_from_params_tuple_str() {
		let mut input = HashMap::new();
		input.insert("p1", "v1");
		input.insert("p2", "v2");
		let params: PathParams<(&str, &str)> = FromParams::from_params(input, &["p1", "p2"]);
		assert_eq!(params.0.0, "v1");
		assert_eq!(params.0.1, "v2");
	}
}
