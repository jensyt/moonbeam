use moonbeam::router::PathParams;
use moonbeam::server::task::testing::execute;
use moonbeam::{Body, Request, Response, route, router};

// --- Handlers ---

#[route]
async fn rest_handler(PathParams(path): PathParams<&str>) -> Response {
	Response::ok().with_body(path, Body::DEFAULT_CONTENT_TYPE)
}

#[route]
async fn mixed_handler(PathParams((id, path)): PathParams<(&str, &str)>) -> Response {
	Response::ok().with_body(
		format!("id: {}, path: {}", id, path),
		Body::DEFAULT_CONTENT_TYPE,
	)
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
	let router = RestRouter::new();

	// Test /static/foo/bar
	let req = Request::new("GET", "/static/foo/bar", &[], &[]);
	execute(
		&router,
		req,
		|res| {
			assert_eq!(res.status, 200);
			// Body should be "foo/bar"
			if let Some(Body::Immediate(data)) = res.body {
				assert_eq!(String::from_utf8_lossy(&data), "foo/bar");
			} else {
				panic!("Expected immediate body");
			}
		},
		|_| {},
	);
}

#[test]
fn test_mixed_rest_param() {
	let router = RestRouter::new();

	// Test /users/123/files/a/b/c
	let req = Request::new("GET", "/users/123/files/a/b/c", &[], &[]);
	execute(
		&router,
		req,
		|res| {
			assert_eq!(res.status, 200);
			// Body should be "id: 123, path: a/b/c"
			if let Some(Body::Immediate(data)) = res.body {
				assert_eq!(String::from_utf8_lossy(&data), "id: 123, path: a/b/c");
			} else {
				panic!("Expected immediate body");
			}
		},
		|_| {},
	);
}

#[test]
fn test_rest_param_with_separators() {
	let router = RestRouter::new();

	// Test /static/foo//bar
	let req = Request::new("GET", "/static/foo//bar", &[], &[]);
	execute(
		&router,
		req,
		|res| {
			assert_eq!(res.status, 200);
			// Body should be "foo//bar" (preserving original separators)
			if let Some(Body::Immediate(data)) = res.body {
				assert_eq!(String::from_utf8_lossy(&data), "foo//bar");
			} else {
				panic!("Expected immediate body");
			}
		},
		|_| {},
	);
}

#[test]
fn test_long_rest_param() {
	let router = RestRouter::new();

	// Test path with > 8 segments to verify the fix for long paths
	// /static/1/2/3/4/5/6/7/8/9/10 (11 segments total)
	let req = Request::new("GET", "/static/1/2/3/4/5/6/7/8/9/10", &[], &[]);
	execute(
		&router,
		req,
		|res| {
			assert_eq!(res.status, 200);

			if let Some(Body::Immediate(data)) = res.body {
				assert_eq!(String::from_utf8_lossy(&data), "1/2/3/4/5/6/7/8/9/10");
			} else {
				panic!("Expected immediate body");
			}
		},
		|_| {},
	);
}
