use moonbeam::{Body, Request, Response, server};

#[server(HelloWorld)]
async fn serve(_request: Request) -> Response {
	Response::new_with_body("Hello, World!", Body::TEXT)
}

fn main() {
	println!("Moonbeam (ST) listening on http://127.0.0.1:3030/");
	moonbeam::serve("127.0.0.1:3030", HelloWorld);
}
