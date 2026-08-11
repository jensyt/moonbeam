use moonbeam::server::task::testing::execute;
use moonbeam::{Body, Request, Response, route, router};

#[route]
async fn get_handler(req: Request) -> Response {
	if req.method == "HEAD" {
		Response::ok().with_body("HEAD processed by GET handler", Body::DEFAULT_CONTENT_TYPE)
	} else {
		Response::ok().with_body("GET processed", Body::DEFAULT_CONTENT_TYPE)
	}
}

#[route]
async fn head_handler(_req: Request) -> Response {
	Response::ok().with_body("HEAD explicit", Body::DEFAULT_CONTENT_TYPE)
}

router! {
	HeadRouter {
		get("/implicit") => get_handler,
		get("/explicit") => get_handler,
		head("/explicit") => head_handler
	}
}

#[test]
fn test_implicit_head() {
	let router = HeadRouter::new();
	let req = Request::new("HEAD", "/implicit", &[], &[]);
	execute(
		&router,
		req,
		|res| {
			assert_eq!(
				res.status, 200,
				"HEAD request should be handled by GET handler if no HEAD handler exists"
			);
			assert_body(res.body, "HEAD processed by GET handler");
		},
		|_| {},
	);
}

#[test]
fn test_explicit_head() {
	let router = HeadRouter::new();
	let req = Request::new("HEAD", "/explicit", &[], &[]);
	execute(
		&router,
		req,
		|res| {
			assert_eq!(res.status, 200);
			assert_body(res.body, "HEAD explicit");
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
