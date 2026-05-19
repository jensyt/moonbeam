use moonbeam::{Body, Request, Response, Server, Spawner};

struct HelloNoMacros(&'static str);
impl Server for HelloNoMacros {
	async fn route<'s: 'e, 'e>(
		&'s self,
		_request: Request<'_, '_>,
		_spawner: Spawner<'e>,
	) -> Response {
		Response::ok().with_body(format!("Hello {}", self.0), Body::TEXT)
	}
}

fn main() {
	println!("Running on 127.0.0.1:7463. Press Ctrl+C to exit");
	moonbeam::serve("127.0.0.1:7463", || HelloNoMacros("No Macros"));
}
