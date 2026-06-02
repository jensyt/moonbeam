use moonbeam::{Body, Request, Response, Server, Spawner};

// Note: we're using 'static here to make the example easier to read, but this could be an arbitrary
// lifetime (e.g. 'a) that allows the server to hold a reference to objects created during
// initialization.
struct HelloNoMacros(&'static str);
impl Server for HelloNoMacros {
	async fn route<'server: 'exec, 'exec>(
		&'server self,
		_request: Request<'_, '_>,
		_spawner: Spawner<'exec>,
	) -> Response {
		Response::ok().with_body(format!("Hello {}", self.0), Body::TEXT)
	}
}

fn main() {
	println!("Running on 127.0.0.1:7463. Press Ctrl+C to exit");
	moonbeam::serve("127.0.0.1:7463", || HelloNoMacros("No Macros"));
}
