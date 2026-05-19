use std::convert::Infallible;

use futures_lite::future::block_on;
use moonbeam::http::{Body, FromRequest, Request};
use moonbeam::{Executor, Response, Server, route, router};

struct State {
	name: String,
}

struct Name<'a>(&'a str);
impl<'s> FromRequest<'_, '_, 's, State> for Name<'s> {
	type Error = Infallible;

	async fn from_request(_req: Request<'_, '_>, state: &'s State) -> Result<Self, Self::Error> {
		Ok(Self(&state.name))
	}
}

#[route(state = State)]
async fn echo_user(Name(user): Name<'_>) -> Response {
	Response::ok().with_body(format!("Hello {user}"), Body::TEXT)
}

router!(StateRouter<State> {
	post("/echo") => echo_user
});

#[test]
fn test_state_extraction() {
	let state = State {
		name: "Jens".to_string(),
	};
	let router = StateRouter::new(state);
	let executor = Executor::new();

	let headers = [];
	let req = Request::new("POST", "/echo", &headers, &[]);

	let res = block_on(router.route(req, executor.spawner()));

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
