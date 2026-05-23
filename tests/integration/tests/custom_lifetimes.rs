use futures_lite::future::block_on;
use moonbeam::{Executor, Request, Response, Server, Spawner, middleware, route, router};
use std::cell::Cell;

struct TestState {
	value: Cell<i32>,
}

#[route]
async fn spawn_custom_lifetimes<'exec, 'state>(
	state: &'state TestState,
	spawner: Spawner<'exec>,
) -> Response {
	spawner.spawn(async move {
		state.value.update(|v| v + 1);
	});
	Response::ok()
}

router! {
	TestRouter<TestState> {
		get("/custom_lifetimes") => spawn_custom_lifetimes,
	}
}

#[test]
fn test_spawn_custom_lifetimes() {
	let state = TestState {
		value: Cell::new(42),
	};
	let router = TestRouter::new(state);
	let executor = Executor::new();

	let headers = [];
	let req = Request::new("GET", "/custom_lifetimes", &headers, &[]);
	let res = block_on(router.route(req, executor.spawner()));
	assert_eq!(res.status, 200);
	assert_eq!(router.0.value.get(), 42);
	assert_eq!(executor.try_tick(), true);
	assert_eq!(router.0.value.get(), 43);
	assert_eq!(executor.try_tick(), false);
}

#[moonbeam::server(CustomLifetimeServer)]
async fn handle_custom_lifetimes<'exec, 'state>(
	_req: Request,
	spawner: Spawner<'exec>,
	state: &'state TestState,
) -> Response {
	spawner.spawn(async move {
		state.value.update(|v| v + 1);
	});
	Response::ok()
}

#[test]
fn test_server_custom_lifetimes() {
	let state = TestState {
		value: Cell::new(42),
	};
	let server = CustomLifetimeServer(state);
	let executor = Executor::new();

	let headers = [];
	let req = Request::new("GET", "/foo", &headers, &[]);
	let res = block_on(server.route(req, executor.spawner()));
	assert_eq!(res.status, 200);
	assert_eq!(server.0.value.get(), 42);
	assert_eq!(executor.try_tick(), true);
	assert_eq!(server.0.value.get(), 43);
	assert_eq!(executor.try_tick(), false);
}

#[middleware]
async fn custom_lifetime_middleware<'req_a, 'req_b, 'exec, F>(
	req: Request<'req_a, 'req_b>,
	spawner: Spawner<'exec>,
	state: &'exec TestState,
	next: Next,
) -> Response {
	spawner.spawn(async move {
		state.value.update(|v| v + 1);
	});
	next(req).await
}

router! {
	TestRouterWithMiddleware<TestState> {
		with custom_lifetime_middleware
		get("/custom_lifetimes") => spawn_custom_lifetimes,
	}
}

#[test]
fn test_middleware_custom_lifetimes() {
	let state = TestState {
		value: Cell::new(42),
	};
	let router = TestRouterWithMiddleware::new(state);
	let executor = Executor::new();

	let headers = [];
	let req = Request::new("GET", "/custom_lifetimes", &headers, &[]);
	let res = block_on(router.route(req, executor.spawner()));
	assert_eq!(res.status, 200);
	assert_eq!(router.0.value.get(), 42);
	// Both middleware and route handler spawn a task that increments the count
	assert_eq!(executor.try_tick(), true); // First task
	assert_eq!(router.0.value.get(), 43);
	assert_eq!(executor.try_tick(), true); // Second task
	assert_eq!(router.0.value.get(), 44);
	assert_eq!(executor.try_tick(), false);
}
