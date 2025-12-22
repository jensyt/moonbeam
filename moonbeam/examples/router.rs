use moonbeam::router::PathParams;
use moonbeam::{Response, route, router, serve};
use std::sync::atomic::{AtomicU32, Ordering};

struct State {
	count: AtomicU32,
}

#[route]
async fn hello(PathParams(map): PathParams, state: &'static State) -> Response {
	let count = state.count.fetch_add(1, Ordering::Relaxed);
	let name = map.get("name").map(|s| s.as_str()).unwrap_or("unknown");
	Response::new_with_body(
		format!("Hello {name}! Request #{count}."),
		Some("text/plain"),
	)
}

#[route]
async fn hello_two(
	PathParams((first, last)): PathParams<(String, String)>,
	state: &'static State,
) -> Response {
	let count = state.count.fetch_add(1, Ordering::Relaxed);
	Response::new_with_body(
		format!("Hello {first} {last}! Request #{count}."),
		Some("text/plain"),
	)
}

router!(MyRouter<State> {
	get("/hello/:name") => hello,
	get("/hello/:first/:last") => hello_two,
});

fn main() {
	let router = MyRouter::new(State {
		count: AtomicU32::new(0),
	});
	println!("Router compiled successfully! Running on 127.0.0.1:5678");
	serve("127.0.0.1:5678", router);
}
