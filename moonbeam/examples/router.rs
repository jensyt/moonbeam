use moonbeam::router::PathParams;
use moonbeam::{Request, Response, route, router, serve};

#[route]
async fn hello(PathParams(map): PathParams, _request: Request<'_, '_>) -> Response {
	let name = map.get("name").map(|s| s.as_str()).unwrap_or("unknown");
	Response::new_with_body(format!("Hello {name}!"), Some("text/plain"))
}

#[route]
async fn hello_two(
	PathParams((first, last)): PathParams<(String, String)>,
	_request: Request<'_, '_>,
) -> Response {
	Response::new_with_body(format!("Hello {first} {last}!"), Some("text/plain"))
}

router!(MyRouter {
	get("/hello/:name") => hello,
	get("/hello/:first/:last") => hello_two,
});

fn main() {
	let router = MyRouter::new(()); // Stateless
	println!("Router compiled successfully! Running on 127.0.0.1:5678");
	serve("127.0.0.1:5678", router);
}
