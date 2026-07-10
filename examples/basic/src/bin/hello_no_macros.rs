use moonbeam::{AsyncFnServer, Body, Request, Response, Server, Spawner, StatelessAsyncFnServer};

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

async fn no_state<'req, 'exec>(
	_request: Request<'req, 'req>,
	_spawner: Spawner<'exec>,
) -> Response<'req> {
	Response::ok().with_body(format!("Hello no state"), Body::TEXT)
}

async fn with_state<'req, 'exec>(
	_request: Request<'req, 'req>,
	_spawner: Spawner<'exec>,
	state: &'exec &str,
) -> Response<'req> {
	Response::ok().with_body(format!("Hello {}", state), Body::TEXT)
}

fn main() {
	let version = if let Some(arg) = std::env::args().nth(1) {
		match arg.as_str() {
			"struct" => 1,
			"fn" => 2,
			"fnstate" => 3,
			_ => 1,
		}
	} else {
		1
	};
	println!("Running on 127.0.0.1:7463. Press Ctrl+C to exit");
	match version {
		2 => moonbeam::serve("127.0.0.1:7463", || StatelessAsyncFnServer::new(no_state)),
		3 => moonbeam::serve("127.0.0.1:7463", || {
			AsyncFnServer::new(with_state, "No Macros")
		}),
		_ => moonbeam::serve("127.0.0.1:7463", || HelloNoMacros("No Macros")),
	};
}
