use futures_lite::future::block_on;
use moonbeam::{Body, Request, Response, Server, route, router};

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
	let router = Box::leak(Box::new(TestRouter::new(state)));

	let headers = [];

	// Test Ok result
	let req = Request::new("GET", "/ok", &headers, &[]);
	let res = block_on(router.route(req));
	assert_eq!(res.status, 200);
	assert_body(res.body, "ok");

	// Test Err result
	let req = Request::new("GET", "/err", &headers, &[]);
	let res = block_on(router.route(req));
	assert_eq!(res.status, 400);
	assert_body(res.body, "error");
}

fn assert_body(body: Option<Body>, expected: &str) {
	match body {
		Some(Body::Immediate(data)) => {
			assert_eq!(String::from_utf8_lossy(&data), expected);
		}
		_ => panic!("Expected immediate body"),
	}
}
