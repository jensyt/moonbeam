use moonbeam::http::FromState;
use moonbeam::{Body, Response, from_request, route, router, serve};
use std::convert::Infallible;
use std::sync::atomic::{AtomicU32, Ordering};

struct Database {
	query_count: AtomicU32,
}

struct AppState {
	db: Database,
}

// A custom extractor that extracts the Database reference from AppState
struct DbConn<'a>(&'a Database);

// The from_request attribute is used to implement FromRequest using the FromState impl, so that it
// can be extracted from the request state.
#[from_request]
impl<'s> FromState<'s, AppState> for DbConn<'s> {
	type Error = Infallible;

	fn from_state(state: &'s AppState) -> Result<Self, Self::Error> {
		Ok(Self(&state.db))
	}
}

// Use #[route(state = AppState)] to specify the state type, since FromState above is only
// implemented for a specific state type (AppState)
#[route(state = AppState)]
async fn query_db(DbConn(db): DbConn<'_>) -> Response {
	let count = db.query_count.fetch_add(1, Ordering::Relaxed);
	Response::new_with_body(format!("Database query count: {}", count + 1), Body::TEXT)
}

fn main() {
	router!(MyRouter<AppState> {
		get("/query") => query_db,
	});

	println!("Running on 127.0.0.1:5678. Press Ctrl+C to exit");
	serve("127.0.0.1:5678", || {
		MyRouter::new(AppState {
			db: Database {
				query_count: AtomicU32::new(0),
			},
		})
	});
}
