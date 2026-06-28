use futures_lite::future::block_on;
use moonbeam::{Executor, Request, Response, Server, Spawner, route, router};
use std::cell::Cell;
use std::pin::pin;

struct TestState {
	value: Cell<i32>,
}

#[route]
async fn spawn_closure(state: &TestState, spawner: Spawner) -> Response {
	spawner.spawn(async {
		state.value.update(|v| v + 1);
	});
	Response::ok()
}

async fn update_func(state: &TestState) {
	state.value.update(|v| v + 1);
}

#[route]
async fn spawn_free(state: &TestState, spawner: Spawner) -> Response {
	spawner.spawn(update_func(state));
	Response::ok()
}

#[route]
async fn spawn_closure2(state: &TestState, spawner: Spawner) -> Response {
	spawner.spawn(async move {
		state.value.update(|v| v + 1);

		spawner.spawn(async {
			state.value.update(|v| v + 1);
		});
	});
	Response::ok()
}

async fn update_func2<'a: 'b, 'b>(state: &'a TestState, spawner: Spawner<'b>) {
	state.value.update(|v| v + 1);

	spawner.spawn(async {
		state.value.update(|v| v + 1);
	});
}

#[route]
async fn spawn_free2(state: &TestState, spawner: Spawner) -> Response {
	spawner.spawn(update_func2(state, spawner));
	Response::ok()
}

router! {
	TestRouter<TestState> {
		get("/closure") => spawn_closure,
		get("/free") => spawn_free,
		get("/closure2") => spawn_closure2,
		get("/free2") => spawn_free2,
	}
}

#[test]
fn test_spawn_closure() {
	let state = TestState {
		value: Cell::new(42),
	};
	let router = TestRouter::new(state);
	let executor = pin!(Executor::new());

	let headers = [];
	let req = Request::new("GET", "/closure", &headers, &[]);
	let res = block_on(router.route(req, executor.as_ref().spawner()));
	assert_eq!(res.status, 200);
	assert_eq!(router.0.value.get(), 42);
	assert_eq!(executor.try_tick(), true);
	assert_eq!(router.0.value.get(), 43);
	assert_eq!(executor.try_tick(), false);
}

#[test]
fn test_spawn_free() {
	let state = TestState {
		value: Cell::new(42),
	};
	let router = TestRouter::new(state);
	let executor = pin!(Executor::new());

	let headers = [];
	let req = Request::new("GET", "/free", &headers, &[]);
	let res = block_on(router.route(req, executor.as_ref().spawner()));
	assert_eq!(res.status, 200);
	assert_eq!(router.0.value.get(), 42);
	assert_eq!(executor.try_tick(), true);
	assert_eq!(router.0.value.get(), 43);
	assert_eq!(executor.try_tick(), false);
}

#[test]
fn test_spawn_closure2() {
	let state = TestState {
		value: Cell::new(42),
	};
	let router = TestRouter::new(state);
	let executor = pin!(Executor::new());

	let headers = [];
	let req = Request::new("GET", "/closure2", &headers, &[]);
	let res = block_on(router.route(req, executor.as_ref().spawner()));
	assert_eq!(res.status, 200);
	assert_eq!(router.0.value.get(), 42);
	assert_eq!(executor.try_tick(), true);
	assert_eq!(router.0.value.get(), 43);
	assert_eq!(executor.try_tick(), true);
	assert_eq!(router.0.value.get(), 44);
	assert_eq!(executor.try_tick(), false);
}

#[test]
fn test_spawn_free2() {
	let state = TestState {
		value: Cell::new(42),
	};
	let router = TestRouter::new(state);
	let executor = pin!(Executor::new());

	let headers = [];
	let req = Request::new("GET", "/free2", &headers, &[]);
	let res = block_on(router.route(req, executor.as_ref().spawner()));
	assert_eq!(res.status, 200);
	assert_eq!(router.0.value.get(), 42);
	assert_eq!(executor.try_tick(), true);
	assert_eq!(router.0.value.get(), 43);
	assert_eq!(executor.try_tick(), true);
	assert_eq!(router.0.value.get(), 44);
	assert_eq!(executor.try_tick(), false);
}
