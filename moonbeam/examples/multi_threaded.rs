#[cfg(feature = "mt")]
mod mt {
	use moonbeam::{Request, Response, ThreadCount, serve_multi, server};
	use std::sync::atomic::{AtomicUsize, Ordering};

	struct State {
		thread_id: usize,
	}

	#[server(Worker)]
	async fn serve(_req: Request, state: &State) -> Response {
		Response::new_with_body(format!("Hello from thread {}", state.thread_id), None)
	}

	pub fn main() {
		let next_id = AtomicUsize::new(0);

		println!("Running on 127.0.0.1:5678. Press Ctrl+C to exit");
		serve_multi(
			"127.0.0.1:5678",
			ThreadCount::Default, // Uses available parallelism
			|| {
				let id = next_id.fetch_add(1, Ordering::Relaxed);
				Worker(State { thread_id: id })
			},
			|_| {}, // No cleanup needed
		);
	}
}
#[cfg(feature = "mt")]
pub use mt::main;

#[cfg(not(feature = "mt"))]
fn main() {
	println!("This example requires feature 'mt'");
}
