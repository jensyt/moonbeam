use futures_lite::future::block_on;
use moonbeam::http::{FromBody, FromState};
use moonbeam::{Body, Executor, Request, Response, Server, from_request, route, router};
use std::convert::Infallible;
use std::pin::pin;
use std::str;

struct State {
	name: String,
}

struct Name<'a>(&'a str);

#[from_request]
impl<'s> FromState<'s, State> for Name<'s> {
	type Error = Infallible;

	fn from_state(state: &'s State) -> Result<Self, Self::Error> {
		Ok(Self(&state.name))
	}
}

#[route(state = State)]
async fn echo_user(Name(user): Name<'_>) -> Response {
	Response::ok().with_body(format!("Hello {user}"), Body::TEXT)
}

struct BodyName<'a>(&'a str);

#[from_request]
impl<'b> FromBody<'b> for BodyName<'b> {
	type Error = Response<'static>;

	fn from_body(body: &'b [u8]) -> Result<Self, Self::Error> {
		str::from_utf8(body)
			.map(BodyName)
			.map_err(|_| Response::bad_request())
	}
}

#[route]
async fn echo_user_body(BodyName(user): BodyName<'_>) -> Response {
	Response::ok().with_body(format!("Hello {user}"), Body::TEXT)
}

router!(StateRouter<State> {
	post("/echo") => echo_user,
	post("/echobody") => echo_user_body
});

#[test]
fn test_state_extraction() {
	let state = State {
		name: "Jens".to_string(),
	};
	let router = StateRouter::new(state);
	let executor = pin!(Executor::new());

	let headers = [];
	let req = Request::new("POST", "/echo", &headers, b"John");

	let res = block_on(router.route(req, executor.as_ref().spawner()));

	assert_eq!(res.status, 200);
	assert!(
		res.headers
			.iter()
			.any(|(n, v)| n.eq_ignore_ascii_case("Content-Type") && v == Body::TEXT.unwrap())
	);

	if let Some(Body::Immediate(data)) = res.body {
		let response_str = String::from_utf8_lossy(&data);
		assert_eq!(response_str, r#"Hello Jens"#);
	} else {
		panic!("Expected immediate body");
	}
}

#[test]
fn test_body_extraction() {
	let state = State {
		name: "Jens".to_string(),
	};
	let router = StateRouter::new(state);
	let executor = pin!(Executor::new());

	let headers = [];
	let req = Request::new("POST", "/echobody", &headers, b"John");

	let res = block_on(router.route(req, executor.as_ref().spawner()));

	assert_eq!(res.status, 200);
	assert!(
		res.headers
			.iter()
			.any(|(n, v)| n.eq_ignore_ascii_case("Content-Type") && v == Body::TEXT.unwrap())
	);

	if let Some(Body::Immediate(data)) = res.body {
		let response_str = String::from_utf8_lossy(&data);
		assert_eq!(response_str, r#"Hello John"#);
	} else {
		panic!("Expected immediate body");
	}
}
