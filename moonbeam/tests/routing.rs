use futures_lite::future::block_on;
use moonbeam::router::PathParams;
use moonbeam::{Body, Request, Response, Server, route, router};

// --- State Definition ---

struct TestState {
	value: i32,
}

// --- Handlers ---

#[route]
async fn index(_req: Request<'_, '_>) -> Response {
	Response::ok().with_body("index", None)
}

#[route]
async fn get_user(PathParams(id): PathParams<&str>) -> Response {
	Response::ok().with_body(format!("user: {}", id), None)
}

#[route]
async fn get_post(PathParams((user_id, post_id)): PathParams<(&str, &str)>) -> Response {
	Response::ok().with_body(format!("user: {}, post: {}", user_id, post_id), None)
}

#[route]
async fn with_state(_req: Request<'_, '_>, state: &'static TestState) -> Response {
	Response::ok().with_body(format!("state: {}", state.value), None)
}

#[route]
async fn create_item(_req: Request<'_, '_>) -> Response {
	Response::new_with_code(201).with_body("created", None)
}

// --- Router Definition ---

router! {
	TestRouter<TestState> {
		get("/") => index,
		get("/users/:id") => get_user,
		get("/users/:user_id/posts/:post_id") => get_post,
		get("/state") => with_state,
		post("/items") => create_item
	}
}

// --- Tests ---

#[test]
fn test_basic_routing() {
	let state = TestState { value: 42 };
	let router = Box::leak(Box::new(TestRouter::new(state)));

	// Test GET /
	let headers = [];
	let req = Request::new("GET", "/", &headers, &[]);
	let res = block_on(router.route(req));
	assert_eq!(res.status, 200);
	assert_body(res.body, "index");
}

#[test]
fn test_path_params() {
	let state = TestState { value: 42 };
	let router = Box::leak(Box::new(TestRouter::new(state)));

	// Test GET /users/123
	let headers = [];
	let req = Request::new("GET", "/users/123", &headers, &[]);
	let res = block_on(router.route(req));
	assert_eq!(res.status, 200);
	assert_body(res.body, "user: 123");
}

#[test]
fn test_multiple_path_params() {
	let state = TestState { value: 42 };
	let router = Box::leak(Box::new(TestRouter::new(state)));

	// Test GET /users/123/posts/456
	let headers = [];
	let req = Request::new("GET", "/users/123/posts/456", &headers, &[]);
	let res = block_on(router.route(req));
	assert_eq!(res.status, 200);
	assert_body(res.body, "user: 123, post: 456");
}

#[test]
fn test_state_access() {
	let state = TestState { value: 42 };
	let router = Box::leak(Box::new(TestRouter::new(state)));

	// Test GET /state
	let headers = [];
	let req = Request::new("GET", "/state", &headers, &[]);
	let res = block_on(router.route(req));
	assert_eq!(res.status, 200);
	assert_body(res.body, "state: 42");
}

#[test]
fn test_method_matching() {
	let state = TestState { value: 42 };
	let router = Box::leak(Box::new(TestRouter::new(state)));

	// Test POST /items
	let headers = [];
	let req = Request::new("POST", "/items", &headers, &[]);
	let res = block_on(router.route(req));
	assert_eq!(res.status, 201);
	assert_body(res.body, "created");

	// Test GET /items (should be 404)
	let req = Request::new("GET", "/items", &headers, &[]);
	let res = block_on(router.route(req));
	assert_eq!(res.status, 404);
}

#[test]
fn test_not_found() {
	let state = TestState { value: 42 };
	let router = Box::leak(Box::new(TestRouter::new(state)));

	// Test non-existent route
	let headers = [];
	let req = Request::new("GET", "/not-found", &headers, &[]);
	let res = block_on(router.route(req));
	assert_eq!(res.status, 404);
}

// Helper to check body content
fn assert_body(body: Option<Body>, expected: &str) {
	match body {
		Some(Body::Immediate(data)) => {
			assert_eq!(String::from_utf8_lossy(&data), expected);
		}
		_ => panic!("Expected immediate body"),
	}
}
