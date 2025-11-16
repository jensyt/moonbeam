use moonbeam::{Request, Response, server, Server};

#[server(HelloWorld)]
async fn serve(_request: Request<'_, '_>) -> Response {
	Response::new_with_body("Hello, World!", Some("text/plain"))
}

fn main() {
	println!("Running on 127.0.0.1:7463. Press Ctrl+C to exit");
	HelloWorld::serve(HelloWorld, "127.0.0.1:7463");
}
