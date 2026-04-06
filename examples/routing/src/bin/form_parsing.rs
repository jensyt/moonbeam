use moonbeam::http::Response;
use moonbeam::{Body, route, router, serve};
use moonbeam_serde::Form;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct User<'a> {
	name: &'a str,
	age: u32,
	active: bool,
}

#[route]
async fn handle_form(Form(user): Form<User<'_>>) -> Response {
	println!("Received form: {:?}", user);
	Response::ok().with_body(
		format!(
			"Hello, {} (age: {}, active: {})!",
			user.name, user.age, user.active
		),
		Body::TEXT,
	)
}

router!(MyRouter {
	post("/submit") => handle_form,
});

fn main() {
	println!("Serving form parsing example on 127.0.0.1:8081");
	println!("Test with:");
	println!("curl -X POST -d \"name=Jens&age=42&active=true\" http://127.0.0.1:8081/submit");
	serve("127.0.0.1:8081", MyRouter);
}
