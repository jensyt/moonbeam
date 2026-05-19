use moonbeam::{Body, Response, Spawner, route, router, serve};
use std::sync::atomic::{AtomicU32, Ordering};

struct State {
	count: AtomicU32,
}

#[route]
async fn spawn(state: &State, spawner: Spawner) -> Response {
	let count = state.count.fetch_add(1, Ordering::Relaxed);
	spawner.spawn(async {
		// Typically you would do something more interesting here, but this print statement proves
		// that this executes AFTER the current count print below it.
		println!("Next count: {}", state.count.load(Ordering::Relaxed));
	});
	println!("Current count: {}", count);
	Response::new_with_body(format!("Request #{count}."), Body::TEXT)
}

fn main() {
	router!(MyRouter<State> {
		get("/") => spawn,
	});

	let router = MyRouter::new(State {
		count: AtomicU32::new(0),
	});
	println!("Running on 127.0.0.1:5678. Press Ctrl+C to exit");
	serve("127.0.0.1:5678", router);
}
