use futures_lite::future::block_on;
use moonbeam::router::PathParams;
use moonbeam::{Body, Request, Response, Server, route, router};

// --- Handlers ---

#[route]
async fn rest_handler(PathParams(path): PathParams<&str>) -> Response {
	Response::ok().with_body(path, None)
}

#[route]
async fn mixed_handler(PathParams((id, path)): PathParams<(&str, &str)>) -> Response {
	Response::ok().with_body(format!("id: {}, path: {}", id, path), None)
}

// --- Router Definition ---

router! {
	RestRouter {
		get("/static/*path") => rest_handler,
		get("/users/:id/files/*path") => mixed_handler
	}
}

// --- Tests ---

#[test]
fn test_rest_param() {
	let router = Box::leak(Box::new(RestRouter::new()));

	// Test /static/foo/bar
	let headers = [];
	let req = Request::new("GET", "/static/foo/bar", &headers, &[]);
	let res = block_on(router.route(req));
	assert_eq!(res.status, 200);
	// Body should be "foo/bar"
	if let Some(Body::Immediate(data)) = res.body {
		assert_eq!(String::from_utf8_lossy(&data), "foo/bar");
	} else {
		panic!("Expected immediate body");
	}
}

#[test]
fn test_mixed_rest_param() {
	let router = Box::leak(Box::new(RestRouter::new()));

	// Test /users/123/files/a/b/c
	let headers = [];
	let req = Request::new("GET", "/users/123/files/a/b/c", &headers, &[]);
	let res = block_on(router.route(req));
	assert_eq!(res.status, 200);
	// Body should be "id: 123, path: a/b/c"
	if let Some(Body::Immediate(data)) = res.body {
		assert_eq!(String::from_utf8_lossy(&data), "id: 123, path: a/b/c");
	} else {
		panic!("Expected immediate body");
	}
}

#[test]
fn test_rest_param_with_separators() {
	let router = Box::leak(Box::new(RestRouter::new()));

	// Test /static/foo//bar
	let headers = [];
	let req = Request::new("GET", "/static/foo//bar", &headers, &[]);
	let res = block_on(router.route(req));
	assert_eq!(res.status, 200);
	// Body should be "foo//bar" (preserving original separators)
	if let Some(Body::Immediate(data)) = res.body {
		assert_eq!(String::from_utf8_lossy(&data), "foo//bar");
	} else {
		panic!("Expected immediate body");
	}
}
