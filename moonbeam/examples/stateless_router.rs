use moonbeam::router::PathParams;
use moonbeam::{Request, Response, route, router, serve};

#[route]
async fn index(req: Request<'_, '_>) -> Response {
	let x = req
		.params()
		.find("x")
		.next()
		.map(|v| format!("Got x = {v}"))
		.unwrap_or_else(|| "Did not get x param".into());
	Response::ok().with_body(
		format!("Welcome to the stateless router! {x}"),
		Some("text/plain"),
	)
}

#[route]
async fn hello(PathParams(name): PathParams<&str>, _req: Request<'_, '_>) -> Response {
	Response::new_with_body(format!("Hello {name}!"), Some("text/plain"))
}

router!(StatelessRouter {
	get("/") => index,
	get("/hello/:name") => hello,
});

fn main() {
	let router = StatelessRouter::stateless();
	println!("Stateless router running on 127.0.0.1:5679");
	serve("127.0.0.1:5679", router);
}
