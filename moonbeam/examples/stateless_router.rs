use moonbeam::router::PathParams;
use moonbeam::{Request, Response, middleware, route, router, serve};

#[route]
async fn index(req: Request) -> Response {
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
fn hello(PathParams(name): PathParams<&str>) -> Response {
	Response::new_with_body(format!("Hello {name}!"), Some("text/plain"))
}

#[middleware]
async fn test<S>(request: Request, _state: &S, next: Next) -> Response {
	next(request).await
}

fn main() {
	router!(
		StatelessRouter {
			with test,

			get("/") => index,
			get("/hello/:name") => hello,
		}
	);

	println!("Running on 127.0.0.1:5679. Press Ctrl+C to exit");
	serve("127.0.0.1:5679", StatelessRouter);
}
