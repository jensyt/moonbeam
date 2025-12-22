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
/// // fn handler(PathParams(params): PathParams<HashMap<String, String>>, ...)
/// ```
///
/// ## Accessing via Tuple Destructuring
/// ```rust
/// use moonbeam::router::PathParams;
///
/// // Handler signature for path "/users/:id/posts/:post_id"
/// // fn handler(PathParams((id, post_id)): PathParams<(String, String)>, ...)
/// ```
#[derive(Debug)]
pub struct PathParams<T = HashMap<String, String>>(pub T);

impl From<HashMap<String, String>> for PathParams<HashMap<String, String>> {
	fn from(params: HashMap<String, String>) -> Self {
		Self(params)
	}
}

/// Trait for converting a raw parameter map into the target `PathParams` type.
/// This allows the `#[route]` macro to automatically convert the map provided by the router
/// into the specific tuple or structure requested by the user.
pub trait FromParams {
	fn from_params(params: HashMap<String, String>, names: &[&str]) -> Self;
}

// Default implementation for HashMap
impl FromParams for PathParams<HashMap<String, String>> {
	fn from_params(params: HashMap<String, String>, _names: &[&str]) -> Self {
		PathParams(params)
	}
}

// Implementations for Tuples (up to 4 for now, can be expanded)
impl FromParams for PathParams<(String,)> {
	fn from_params(mut params: HashMap<String, String>, names: &[&str]) -> Self {
		let p1 = params.remove(names[0]).unwrap_or_default();
		PathParams((p1,))
	}
}

impl FromParams for PathParams<(String, String)> {
	fn from_params(mut params: HashMap<String, String>, names: &[&str]) -> Self {
		let p1 = params.remove(names[0]).unwrap_or_default();
		let p2 = params.remove(names[1]).unwrap_or_default();
		PathParams((p1, p2))
	}
}

impl FromParams for PathParams<(String, String, String)> {
	fn from_params(mut params: HashMap<String, String>, names: &[&str]) -> Self {
		let p1 = params.remove(names[0]).unwrap_or_default();
		let p2 = params.remove(names[1]).unwrap_or_default();
		let p3 = params.remove(names[2]).unwrap_or_default();
		PathParams((p1, p2, p3))
	}
}

impl FromParams for PathParams<(String, String, String, String)> {
	fn from_params(mut params: HashMap<String, String>, names: &[&str]) -> Self {
		let p1 = params.remove(names[0]).unwrap_or_default();
		let p2 = params.remove(names[1]).unwrap_or_default();
		let p3 = params.remove(names[2]).unwrap_or_default();
		let p4 = params.remove(names[3]).unwrap_or_default();
		PathParams((p1, p2, p3, p4))
	}
}
