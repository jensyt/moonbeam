use futures_lite::future::block_on;
use moonbeam::http::{Body, Request};
use moonbeam::{Server, route, router};
use moonbeam_serde::Json;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct User<'a> {
	id: u32,
	name: &'a str,
}

#[route]
async fn echo_user(Json(mut user): Json<User<'_>>) -> Json<User<'_>> {
	user.id += 1;
	Json(user)
}

router!(JsonRouter {
	post("/echo") => echo_user
});

#[test]
fn test_json_extraction_borrowed() {
	let router = Box::leak(Box::new(JsonRouter::new()));

	let body_content = r#"{"id": 42, "name": "Jens"}"#;
	let headers = [];
	let req = Request::new("POST", "/echo", &headers, body_content.as_bytes());

	let res = block_on(router.route(req));

	assert_eq!(res.status, 200);
	assert!(
		res.headers
			.iter()
			.any(|(n, v)| n.eq_ignore_ascii_case("Content-Type") && v == "application/json")
	);

	if let Some(Body::Immediate(data)) = res.body {
		let response_str = String::from_utf8_lossy(&data);
		assert_eq!(response_str, r#"{"id":43,"name":"Jens"}"#);
	} else {
		panic!("Expected immediate body");
	}
}
