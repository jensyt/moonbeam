use moonbeam::http::Response;
use moonbeam::{route, router, serve};
use moonbeam_serde::Json;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct User<'a> {
	id: u32,
	name: &'a str, // Borrowed from the request body
}

#[route]
async fn create_user(Json(user): Json<User<'_>>) -> Json<User<'_>> {
	println!("Creating user: {:?}", user);
	Json(user)
}

#[route]
async fn get_user() -> Response {
	let user = User {
		id: 1,
		name: "Jens",
	};
	Json(user).into()
}

router!(MyRouter {
	post("/users") => create_user,
	get("/users/1") => get_user,
});

fn main() {
	println!("Serving JSON API on 127.0.0.1:8080");
	serve("127.0.0.1:8080", MyRouter);
}
