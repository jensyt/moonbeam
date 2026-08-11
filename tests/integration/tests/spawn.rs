use moonbeam::server::task::testing::execute;
use moonbeam::{Request, Response, Spawner, route, router};
use std::cell::Cell;

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

	let req = Request::new("GET", "/closure", &[], &[]);
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

#[test]
fn test_spawn_free() {
	let state = TestState {
		value: Cell::new(42),
	};
	let router = TestRouter::new(state);

	let req = Request::new("GET", "/free", &[], &[]);
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

#[test]
fn test_spawn_closure2() {
	let state = TestState {
		value: Cell::new(42),
	};
	let router = TestRouter::new(state);

	let req = Request::new("GET", "/closure2", &[], &[]);
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
			assert_eq!(tick.try_tick(), true);
			assert_eq!(router.0.value.get(), 44);
			assert_eq!(tick.try_tick(), false);
		},
	);
}

#[test]
fn test_spawn_free2() {
	let state = TestState {
		value: Cell::new(42),
	};
	let router = TestRouter::new(state);

	let req = Request::new("GET", "/free2", &[], &[]);
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
			assert_eq!(tick.try_tick(), true);
			assert_eq!(router.0.value.get(), 44);
			assert_eq!(tick.try_tick(), false);
		},
	);
}
