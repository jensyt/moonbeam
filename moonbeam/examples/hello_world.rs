use moonbeam::{Body, Request, Response, server};

#[server(HelloWorld)]
async fn serve(_request: Request) -> Response {
	Response::new_with_body("Hello, World!", Body::TEXT)
}

fn main() {
	println!("Running on 127.0.0.1:7463. Press Ctrl+C to exit");
	moonbeam::serve("127.0.0.1:7463", HelloWorld);
}
