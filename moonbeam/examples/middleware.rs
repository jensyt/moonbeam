use moonbeam::http::Response;
use moonbeam::router::PathParams;
use moonbeam::{middleware, route, router, serve};

struct State {
	api_key: String,
}

#[middleware]
async fn logger(req: Request, _state: &State, next: Next) -> Response {
	println!("Log: {} {}", req.method, req.url());
	next(req).await
}

#[middleware]
async fn auth(req: Request, state: &State, next: Next) -> Response {
	if let Some(auth_header) = req.find_header("Authorization") {
		if auth_header == state.api_key.as_bytes() {
			return next(req).await;
		}
	}
	Response::new_with_code(401).with_body("Unauthorized", Some("text/plain"))
}

#[route]
async fn public_index(_state: &State) -> Response {
	Response::new_with_body("Public Index", Some("text/plain"))
}

#[route]
async fn api_index(PathParams(id): PathParams<&str>, _state: &State) -> Response {
	Response::new_with_body(format!("API Index for {}", id), Some("text/plain"))
}

#[route]
async fn api_save(_state: &State) -> Response {
	Response::new_with_body("Saved", Some("text/plain"))
}

#[route]
async fn api_v1_status(_state: &State) -> Response {
	Response::new_with_body("V1 Status OK", Some("text/plain"))
}

#[route]
async fn not_found(_state: &State) -> Response {
	Response::new_with_code(404).with_body("Custom Not Found", Some("text/plain"))
}

mod api {
	use moonbeam::{Response, route};
	#[route]
	pub async fn version() -> Response {
		Response::new_with_body("1.0.0", Some("text/plain"))
	}
}

fn main() {
	router!(MyRouter<State> {
		with logger

		get("/") => public_index,
		get("/version") => api::version,

		"/api" => {
			with auth

			get("/:id") => api_index,
			post("/save") with logger => api_save,

			"/v1" => {
				get("/status") => api_v1_status,
			}
			_ => !
		}

		_ => not_found
	});

	let router = MyRouter::new(State {
		api_key: "secret".to_string(),
	});
	println!("Running on 127.0.0.1:5678. Press Ctrl+C to exit");
	serve("127.0.0.1:5678", router);
}
