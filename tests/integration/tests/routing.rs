use moonbeam::{
	Body, Request, Response, route, router, router::PathParams, server::task::testing::execute,
};

// --- State Definition ---

struct TestState {
	value: i32,
}

// --- Handlers ---

#[route]
async fn index(_req: Request) -> Response {
	Response::ok().with_body("index", Body::DEFAULT_CONTENT_TYPE)
}

#[route]
async fn get_user(PathParams(id): PathParams<&str>) -> Response {
	Response::ok().with_body(format!("user: {}", id), Body::DEFAULT_CONTENT_TYPE)
}

#[route]
async fn get_post(PathParams((user_id, post_id)): PathParams<(&str, &str)>) -> Response {
	Response::ok().with_body(
		format!("user: {}, post: {}", user_id, post_id),
		Body::DEFAULT_CONTENT_TYPE,
	)
}

#[route]
async fn with_state(_req: Request, state: &TestState) -> Response {
	Response::ok().with_body(
		format!("state: {}", state.value),
		Body::DEFAULT_CONTENT_TYPE,
	)
}

#[route]
async fn create_item(_req: Request) -> Response {
	Response::new_with_code(201).with_body("created", Body::DEFAULT_CONTENT_TYPE)
}

#[route]
async fn grouped_handler(_req: Request) -> Response {
	Response::ok().with_body("grouped", Body::DEFAULT_CONTENT_TYPE)
}

#[route]
async fn custom_catchall(_req: Request) -> Response {
	Response::ok().with_body("custom catchall", Body::DEFAULT_CONTENT_TYPE)
}

// --- Router Definition ---

router! {
	TestRouter<TestState> {
		get("/") => index,
		get("/users/:id") => get_user,
		get("/users/:user_id/posts/:post_id") => get_post,
		get("/state") => with_state,
		post("/items") => create_item,
		get("/a/b/c/d/e/f/g/h") => index,
		// Note the route below should never match because it is 9 segments
		get("/a/b/c/d/e/f/g/h/extra") => index,
	}
}

router! {
	PrefixlessGroupRouter {
		get("/") => index,
		{
			get("/grouped") => grouped_handler
		}
	}
}

router! {
	NestedCatchallRouter {
		get("/") => index,
		{
			_ => custom_catchall
		}
	}
}

// --- Tests ---

#[test]
fn test_basic_routing() {
	let state = TestState { value: 42 };
	let router = TestRouter::new(state);

	// Test GET /
	let req = Request::new("GET", "/", &[], &[]);
	execute(
		&router,
		req,
		|res| {
			assert_eq!(res.status, 200);
			assert_body(res.body, "index");
		},
		|_| {},
	);
}

#[test]
fn test_path_params() {
	let state = TestState { value: 42 };
	let router = TestRouter::new(state);

	// Test GET /users/123
	let req = Request::new("GET", "/users/123", &[], &[]);
	execute(
		&router,
		req,
		|res| {
			assert_eq!(res.status, 200);
			assert_body(res.body, "user: 123");
		},
		|_| {},
	);
}

#[test]
fn test_multiple_path_params() {
	let state = TestState { value: 42 };
	let router = TestRouter::new(state);

	// Test GET /users/123/posts/456
	let req = Request::new("GET", "/users/123/posts/456", &[], &[]);
	execute(
		&router,
		req,
		|res| {
			assert_eq!(res.status, 200);
			assert_body(res.body, "user: 123, post: 456");
		},
		|_| {},
	);
}

#[test]
fn test_state_access() {
	let state = TestState { value: 42 };
	let router = TestRouter::new(state);

	// Test GET /state
	let req = Request::new("GET", "/state", &[], &[]);
	execute(
		&router,
		req,
		|res| {
			assert_eq!(res.status, 200);
			assert_body(res.body, "state: 42");
		},
		|_| {},
	);
}

#[test]
fn test_method_matching() {
	let state = TestState { value: 42 };
	let router = TestRouter::new(state);

	// Test POST /items
	let req = Request::new("POST", "/items", &[], &[]);
	execute(
		&router,
		req,
		|res| {
			assert_eq!(res.status, 201);
			assert_body(res.body, "created");
		},
		|_| {},
	);

	// Test GET /items (should be 405 Method Not Allowed)
	let req = Request::new("GET", "/items", &[], &[]);
	execute(
		&router,
		req,
		|res| {
			assert_eq!(res.status, 405);
			assert_eq!(
				res.headers
					.iter()
					.find(|(n, _)| n.eq_ignore_ascii_case("Allow"))
					.unwrap()
					.1,
				"POST"
			);
		},
		|_| {},
	);
}

#[test]
fn test_not_found() {
	let state = TestState { value: 42 };
	let router = TestRouter::new(state);

	// Test non-existent route
	let req = Request::new("GET", "/not-found", &[], &[]);
	execute(
		&router,
		req,
		|res| {
			assert_eq!(res.status, 404);
		},
		|_| {},
	);
}

#[test]
fn test_route_overflow_not_matching_8_segments() {
	let state = TestState { value: 42 };
	let router = TestRouter::new(state);

	// Exactly 8 segments matches
	let req = Request::new("GET", "/a/b/c/d/e/f/g/h", &[], &[]);
	execute(
		&router,
		req,
		|res| {
			assert_eq!(res.status, 200);
		},
		|_| {},
	);

	// 9 segments should NOT match the 8-segment or 9-segment route
	let req = Request::new("GET", "/a/b/c/d/e/f/g/h/extra", &[], &[]);
	execute(
		&router,
		req,
		|res| {
			assert_eq!(res.status, 404);
		},
		|_| {},
	);
}

#[test]
fn test_prefixless_group() {
	let router = PrefixlessGroupRouter::new();

	let req = Request::new("GET", "/grouped", &[], &[]);
	execute(
		&router,
		req,
		|res| {
			assert_eq!(res.status, 200);
			assert_body(res.body, "grouped");
		},
		|_| {},
	);
}

#[test]
fn test_nested_catchall() {
	let router = NestedCatchallRouter::new();

	let req = Request::new("GET", "/not-found-here", &[], &[]);
	execute(
		&router,
		req,
		|res| {
			assert_eq!(res.status, 200);
			assert_body(res.body, "custom catchall");
		},
		|_| {},
	);
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
