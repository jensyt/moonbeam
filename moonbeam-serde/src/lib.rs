#![doc = include_str!("../README.md")]

use moonbeam::http::{FromBody, FromRequest};
use moonbeam::{Body, Request, Response, SseEvent};
use serde::Serialize;
use serde::de::Deserialize;

mod forms;
pub use forms::{File, Form};

/// A wrapper for JSON request and response bodies.
///
/// This struct implements `FromBody`, allowing it to be used as an extractor
/// in route handlers. It also implements `Into<Response>` for easy response generation.
///
/// # Example
///
/// ```rust,no_run
/// use moonbeam::{route, Response, Body};
/// use moonbeam_serde::Json;
/// use serde::{Serialize, Deserialize};
/// use std::borrow::Cow;
///
/// #[derive(Debug, Serialize, Deserialize)]
/// struct User<'a> {
///     #[serde(borrow)]
///     name: Cow<'a, str>,
/// }
///
/// #[route]
/// async fn create_user(Json(user): Json<User<'_>>) -> Json<User<'_>> {
///     Json(user)
/// }
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Json<T>(pub T);

impl<'buf, T: Deserialize<'buf>> FromBody<'buf> for Json<T> {
	type Error = Response<'static>;

	fn from_body(body: &'buf [u8]) -> Result<Self, Self::Error> {
		serde_json::from_slice(body)
			.map(Json)
			.map_err(|_| Response::bad_request())
	}
}

impl<'buf, T: Deserialize<'buf>, State> FromRequest<'_, 'buf, '_, State> for Json<T> {
	type Error = Response<'static>;

	async fn from_request(req: Request<'_, 'buf>, _state: &State) -> Result<Self, Self::Error> {
		Self::from_body(req.body)
	}
}

impl<T: Serialize> From<Json<T>> for Response<'static> {
	fn from(json: Json<T>) -> Self {
		match serde_json::to_vec(&json.0) {
			Ok(body) => Response::ok().with_body(body, Body::JSON),
			Err(_) => Response::internal_server_error(),
		}
	}
}

impl<T> From<T> for Json<T> {
	fn from(val: T) -> Self {
		Json(val)
	}
}

/// A trait for adding JSON data to a response or event.
pub trait WithJsonData {
	/// Serializes the provided value to JSON and associates it as data.
	fn with_json_data(self, json: impl Serialize) -> Self;
}

impl WithJsonData for SseEvent {
	fn with_json_data(self, json: impl Serialize) -> Self {
		let data = serde_json::to_string(&json).unwrap_or_default();
		self.with_data(data)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use moonbeam::http::Body;
	use serde::Deserialize;

	#[derive(Debug, Serialize, Deserialize, PartialEq)]
	struct User<'a> {
		id: u32,
		name: &'a str,
	}

	#[test]
	fn test_json_from_body_borrowed() {
		let body = r#"{"id": 42, "name": "Jens"}"#.as_bytes();
		let Json(user): Json<User<'_>> = FromBody::from_body(body).unwrap();

		assert_eq!(user.id, 42);
		assert_eq!(user.name, "Jens");
	}

	#[test]
	fn test_json_from_body_invalid() {
		let body = r#"{"id": "not-a-number"}"#.as_bytes();
		let res: Result<Json<User<'_>>, Response> = FromBody::from_body(body);

		assert!(res.is_err());
		assert_eq!(res.unwrap_err().status, 400);
	}

	#[test]
	fn test_json_into_response() {
		let user = User {
			id: 1,
			name: "Jens",
		};
		let resp: Response = Json(user).into();

		assert_eq!(resp.status, 200);
		assert!(
			resp.headers
				.iter()
				.any(|(n, v)| n.eq_ignore_ascii_case("Content-Type") && v == "application/json")
		);

		if let Some(Body::Immediate(data)) = resp.body {
			assert_eq!(String::from_utf8_lossy(&data), r#"{"id":1,"name":"Jens"}"#);
		} else {
			panic!("Expected immediate body");
		}
	}
}
