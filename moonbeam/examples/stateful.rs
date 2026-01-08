use moonbeam::{Request, Response, server};
use std::cell::RefCell;
use std::sync::atomic::{AtomicUsize, Ordering};

struct State {
	// Atomic counter (thread-safe, though we are single-threaded)
	count: AtomicUsize,
	// Complex state using interior mutability.
	// We use RefCell because the server is single-threaded, so we don't need
	// thread-safe locking like Mutex or RwLock.
	//
	// Note: Be careful not to hold RefCell borrows across .await points!
	request_log: RefCell<Vec<String>>,
}

#[server(StatefulServer)]
async fn handle_request(req: Request, state: &State) -> Response {
	// Update atomic counter
	let count = state.count.fetch_add(1, Ordering::Relaxed);

	// Update complex state
	// We can safely borrow_mut because of the single-threaded runtime.
	// We scope this block to ensure the mutable borrow is dropped before any potential await.
	{
		let mut log = state.request_log.borrow_mut();
		log.push(req.path.to_string());
		// Limit log size to prevent memory leak in this example
		if log.len() > 10 {
			log.remove(0);
		}
	}

	let log_output = {
		let log = state.request_log.borrow();
		format!("{:?}", *log)
	};

	let body = format!(
		"Total requests: {}\nRecent paths: {}\n",
		count + 1,
		log_output
	);

	Response::new_with_body(body, Some("text/plain"))
}

fn main() {
	let state = State {
		count: AtomicUsize::new(0),
		request_log: RefCell::new(Vec::new()),
	};
	println!("Running on 127.0.0.1:7464. Press Ctrl+C to exit");
	moonbeam::serve("127.0.0.1:7464", StatefulServer(state));
}
