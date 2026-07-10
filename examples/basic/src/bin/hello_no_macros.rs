use moonbeam::{Body, Request, Response, Server, Spawner};

// Note: we're using 'static here to make the example easier to read, but this could be an arbitrary
// lifetime (e.g. 'a) that allows the server to hold a reference to objects created during
// initialization.
struct HelloNoMacros(&'static str);
impl Server for HelloNoMacros {
	async fn route<'exec: 'req, 'req>(
		&'exec self,
		_request: Request<'req, 'req>,
		_spawner: Spawner<'exec>,
	) -> Response<'req> {
		Response::ok().with_body(format!("Hello {}", self.0), Body::TEXT)
	}
}

fn main() {
	println!("Running on 127.0.0.1:7463. Press Ctrl+C to exit");
	moonbeam::serve("127.0.0.1:7463", || HelloNoMacros("No Macros"));
}
