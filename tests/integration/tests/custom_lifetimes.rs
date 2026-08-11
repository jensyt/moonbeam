use moonbeam::server::task::testing::execute;
use moonbeam::{Request, Response, Spawner, middleware, route, router};
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

	let req = Request::new("GET", "/custom_lifetimes", &[], &[]);
	execute(
		&router,
		req,
		|res| {
			assert_eq!(res.status, 200);
		},
		|tick| {
			assert_eq!(router.0.value.get(), 42);
			assert_eq!(tick.try_tick(), true);
			assert_eq!(router.0.value.get(), 43);
			assert_eq!(tick.try_tick(), false);
		},
	);
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

	let req = Request::new("GET", "/foo", &[], &[]);
	execute(
		&server,
		req,
		|res| {
			assert_eq!(res.status, 200);
		},
		|tick| {
			assert_eq!(server.0.value.get(), 42);
			assert_eq!(tick.try_tick(), true);
			assert_eq!(server.0.value.get(), 43);
			assert_eq!(tick.try_tick(), false);
		},
	);
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

	let req = Request::new("GET", "/custom_lifetimes", &[], &[]);
	execute(
		&router,
		req,
		|res| {
			assert_eq!(res.status, 200);
		},
		|tick| {
			assert_eq!(router.0.value.get(), 42);
			// Both middleware and route handler spawn a task that increments the count
			assert_eq!(tick.try_tick(), true); // First task
			assert_eq!(router.0.value.get(), 43);
			assert_eq!(tick.try_tick(), true); // Second task
			assert_eq!(router.0.value.get(), 44);
			assert_eq!(tick.try_tick(), false);
		},
	);
}
