use moonbeam::router::PathParams;
use moonbeam::{Request, Response, route, router, serve};

#[route]
async fn index(_req: Request<'_, '_>) -> Response {
	Response::ok().with_body("Welcome to the stateless router!", Some("text/plain"))
}

#[route]
async fn hello(PathParams(map): PathParams, _req: Request<'_, '_>) -> Response {
	let name = map.get("name").map(|s| s.as_str()).unwrap_or("stranger");
	Response::ok().with_body(format!("Hello, {}!", name), Some("text/plain"))
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
