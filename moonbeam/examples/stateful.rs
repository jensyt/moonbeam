use moonbeam::{Request, Response, server};
use std::sync::atomic::{AtomicUsize, Ordering};

struct State {
	count: AtomicUsize,
}

#[server(StatefulServer)]
async fn handle_request(_req: Request<'_, '_>, state: &State) -> Response {
	let count = state.count.fetch_add(1, Ordering::Relaxed);
	Response::new_with_body(format!("Request count: {}", count), Some("text/plain"))
}

fn main() {
	let state = State {
		count: AtomicUsize::new(0),
	};
	println!("Running on 127.0.0.1:7464. Press Ctrl+C to exit");
	moonbeam::serve("127.0.0.1:7464", StatefulServer(state));
}
