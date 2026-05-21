use moonbeam::{Body, Request, Response, Spawner, server};
use std::cell::Cell;

struct State {
	count: Cell<u32>,
}

#[server(SpawningServer)]
async fn handle_request(_req: Request, spawner: Spawner, state: &State) -> Response {
	// Get current counter value
	let count = state.count.get();

	let body = format!("Current count: {}\n", count,);

	// Spawn a task to update the count - this would typically be a longer-running operation but
	// done here for illustrative purposes only.
	spawner.spawn(async move {
		// Note that the async closure can reference state! The `move` above is for `count`, not
		// `state`.
		state.count.set(count + 1);
	});

	Response::new_with_body(body, Body::TEXT)
}

fn main() {
	println!("Running on 127.0.0.1:7464. Press Ctrl+C to exit");
	moonbeam::serve("127.0.0.1:7464", || {
		SpawningServer(State {
			count: Cell::new(0),
		})
	});
}
