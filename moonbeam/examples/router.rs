use moonbeam::router::PathParams;
use moonbeam::{Response, route, router, serve};
use std::sync::atomic::{AtomicU32, Ordering};

struct State {
	count: AtomicU32,
}

#[route]
async fn hello(PathParams(name): PathParams<&str>, state: &'static State) -> Response {
	let count = state.count.fetch_add(1, Ordering::Relaxed);
	Response::new_with_body(
		format!("Hello {name}! Request #{count}."),
		Some("text/plain"),
	)
}

#[route]
async fn hello_two(
	PathParams((first, last)): PathParams<(&str, &str)>,
	state: &'static State,
) -> Response {
	let count = state.count.fetch_add(1, Ordering::Relaxed);
	Response::new_with_body(
		format!("Hello {first} {last}! Request #{count}."),
		Some("text/plain"),
	)
}

#[route]
async fn files(PathParams(path): PathParams<&str>) -> Response {
	Response::new_with_body(format!("Serving file: {path}"), Some("text/plain"))
}

fn main() {
	router!(MyRouter<State> {
		get("/hello/:name") => hello,
		get("/hello/:first/:last") => hello_two,
		get("/static/*path") => files,
	});

	let router = MyRouter::new(State {
		count: AtomicU32::new(0),
	});
	println!("Running on 127.0.0.1:5678. Press Ctrl+C to exit");
	serve("127.0.0.1:5678", router);
}
