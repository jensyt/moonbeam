use moonbeam::router::PathParams;
use moonbeam::{Body, Response, Spawner, route, router, serve, spawn_with_span};
use std::time::Duration;

struct State;

#[route]
async fn hello() -> Response {
	// Any log messages recorded within the route handler are automatically associated with the
	// "handler" span and parent "request" span
	tracing::info!("Received a request to the hello endpoint");
	Response::new_with_body("Hello with Tracing!\n", Body::TEXT)
}

#[route]
async fn delay(PathParams(ms_str): PathParams<&str>) -> Response {
	let ms = ms_str.parse::<u64>().unwrap_or(200);
	tracing::info!(duration_ms = ms, "Delaying response start");

	// Simulate some asynchronous work using `async-io::Timer`.
	// The async-io library integrates nicely with moonbeam's single-threaded async loop.
	async_io::Timer::after(Duration::from_millis(ms)).await;

	tracing::info!(duration_ms = ms, "Delaying response completed");
	Response::new_with_body(format!("Delayed for {}ms\n", ms), Body::TEXT)
}

#[route]
async fn task(spawner: Spawner) -> Response {
	tracing::info!("Preparing to spawn a background task");

	// We use the `spawn_with_span!` macro to spawn the task. This automatically creates a child
	// span named "spawned_task" with a metadata field `task="background_task"`, inheriting the
	// parent request context. We can also pass custom logging fields like `iterations = 1`
	spawn_with_span!(
		spawner,
		"background_task",
		async move {
			tracing::info!("Background task has started running");
			async_io::Timer::after(Duration::from_millis(200)).await;
			tracing::info!("Background task has completed successfully");
		},
		iterations = 1
	);

	tracing::info!("Background task spawned successfully");
	Response::new_with_body("Background task spawned\n", Body::TEXT)
}

fn main() {
	// Initialize the tracing subscriber.
	// We configure a formatter that displays timestamps, log levels, and spans, and set the filter
	// level based on the `RUST_LOG` environment variable, defaulting to trace level for moonbeam
	// and this example, and info level otherwise.
	tracing_subscriber::fmt()
		.with_env_filter(
			tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
				tracing_subscriber::EnvFilter::new("info,moonbeam=trace,examples_tracing=trace")
			}),
		)
		.init();

	router!(MyRouter<State> {
		get("/") => hello,
		get("/delay/:ms") => delay,
		get("/task") => task,
	});

	tracing::info!("Initializing moonbeam server on 127.0.0.1:8000");
	tracing::info!("Test endpoints:");
	tracing::info!("  - http://127.0.0.1:8000/");
	tracing::info!("  - http://127.0.0.1:8000/delay/500");
	tracing::info!("  - http://127.0.0.1:8000/task");

	serve("127.0.0.1:8000", || MyRouter::new(State));
}
