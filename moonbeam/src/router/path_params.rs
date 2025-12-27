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
	fn from_params(params: &[(&'a str, &'a str)], names: &[&str]) -> Self;
}

// Default implementation for HashMap<&str, &str>
impl<'a> FromParams<'a> for PathParamsMap<'a> {
	fn from_params(params: &[(&'a str, &'a str)], _names: &[&str]) -> Self {
		PathParams(params.iter().cloned().collect())
	}
}

fn find_param<'a>(params: &[(&'a str, &'a str)], name: &str) -> &'a str {
	params
		.iter()
		.find(|(k, _)| *k == name)
		.map(|(_, v)| *v)
		.unwrap_or_default()
}

impl<'a> FromParams<'a> for PathParams<&'a str> {
	fn from_params(params: &[(&'a str, &'a str)], names: &[&str]) -> Self {
		let p1 = find_param(params, names[0]);
		PathParams(p1)
	}
}

// Implementations for Tuples (up to 4 for now, can be expanded)
impl<'a> FromParams<'a> for PathParams<(&'a str,)> {
	fn from_params(params: &[(&'a str, &'a str)], names: &[&str]) -> Self {
		let p1 = find_param(params, names[0]);
		PathParams((p1,))
	}
}

impl<'a> FromParams<'a> for PathParams<(&'a str, &'a str)> {
	fn from_params(params: &[(&'a str, &'a str)], names: &[&str]) -> Self {
		let p1 = find_param(params, names[0]);
		let p2 = find_param(params, names[1]);
		PathParams((p1, p2))
	}
}

impl<'a> FromParams<'a> for PathParams<(&'a str, &'a str, &'a str)> {
	fn from_params(params: &[(&'a str, &'a str)], names: &[&str]) -> Self {
		let p1 = find_param(params, names[0]);
		let p2 = find_param(params, names[1]);
		let p3 = find_param(params, names[2]);
		PathParams((p1, p2, p3))
	}
}

impl<'a> FromParams<'a> for PathParams<(&'a str, &'a str, &'a str, &'a str)> {
	fn from_params(params: &[(&'a str, &'a str)], names: &[&str]) -> Self {
		let p1 = find_param(params, names[0]);
		let p2 = find_param(params, names[1]);
		let p3 = find_param(params, names[2]);
		let p4 = find_param(params, names[3]);
		PathParams((p1, p2, p3, p4))
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_from_params_hashmap_str() {
		let input = [("key", "value")];
		let params: PathParams<HashMap<&str, &str>> = FromParams::from_params(&input, &[]);
		assert_eq!(params.0.get("key"), Some(&"value"));
	}

	#[test]
	fn test_from_params_tuple_str() {
		let input = [("p1", "v1"), ("p2", "v2")];
		let params: PathParams<(&str, &str)> = FromParams::from_params(&input, &["p1", "p2"]);
		assert_eq!(params.0.0, "v1");
		assert_eq!(params.0.1, "v2");
	}
}
