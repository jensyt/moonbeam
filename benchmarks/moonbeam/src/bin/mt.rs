use moonbeam::{Body, Request, Response, Spawner, ThreadCount, serve_multi, server};

#[server(HelloWorld)]
async fn serve(_req: Request, _spawner: Spawner) -> Response {
	Response::new_with_body("Hello, World!", Body::TEXT)
}

pub fn main() {
	println!("Moonbeam (MT) listening on http://127.0.0.1:3030/");
	serve_multi(
		"127.0.0.1:3030",
		ThreadCount::Count(4),
		|| HelloWorld,
	);
}
