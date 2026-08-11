use moonbeam::server::task::testing::execute;
use moonbeam::{Body, Request, Response, route, router};

struct TestState;

#[route]
async fn ok_handler(_req: Request) -> Result<Response, Response> {
	Ok(Response::ok().with_body("ok", Body::DEFAULT_CONTENT_TYPE))
}

#[route]
async fn err_handler(_req: Request) -> Result<Response, Response> {
	Err(Response::bad_request().with_body("error", Body::DEFAULT_CONTENT_TYPE))
}

router! {
	TestRouter<TestState> {
		get("/ok") => ok_handler,
		get("/err") => err_handler
	}
}

#[test]
fn test_result_handlers() {
	let state = TestState;
	let router = TestRouter::new(state);

	// Test Ok result
	let req = Request::new("GET", "/ok", &[], &[]);
	execute(
		&router,
		req,
		|res| {
			assert_eq!(res.status, 200);
			assert_body(res.body, "ok");
		},
		|_| {},
	);

	// Test Err result
	let req = Request::new("GET", "/err", &[], &[]);
	execute(
		&router,
		req,
		|res| {
			assert_eq!(res.status, 400);
			assert_body(res.body, "error");
		},
		|_| {},
	);
}

fn assert_body(body: Option<Body>, expected: &str) {
	match body {
		Some(Body::Immediate(data)) => {
			assert_eq!(String::from_utf8_lossy(&data), expected);
		}
		_ => panic!("Expected immediate body"),
	}
}
