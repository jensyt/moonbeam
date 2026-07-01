use futures_lite::{AsyncReadExt, AsyncWriteExt, future::block_on};
use moonbeam::{Body, Executor, Request, Response, Server, route, router};
use piper::pipe;
use std::pin::pin;

#[route]
async fn asyncread_handler() -> Response {
	let (reader, mut writer) = pipe(1024);
	let _ = std::thread::spawn(move || {
		block_on(async move {
			writer.write_all(b"async").await.unwrap();
			writer.write_all(b" stream").await.unwrap();
		});
	});

	Response::ok().with_body(Body::from_async_read(reader), Body::TEXT)
}

#[route]
async fn asyncstreamfn_handler() -> Response {
	Response::ok().with_body(
		Body::from_stream_fn(async |writer| {
			writer.write(b"async stream").await;
		}),
		Body::TEXT,
	)
}

router! {
	TestRouter {
		get("/asyncread") => asyncread_handler,
		get("/asyncstreamfn") => asyncstreamfn_handler
	}
}

#[test]
fn test_async_read_response() {
	let router = TestRouter::new();
	let executor = pin!(Executor::new());

	let headers = [];
	let req = Request::new("GET", "/asyncread", &headers, &[]);
	let mut res = block_on(router.route(req, executor.as_ref().spawner()));

	assert_eq!(res.status, 200);

	let body = res.body.take().unwrap();
	match body {
		Body::AsyncStream { mut data, len } => {
			assert_eq!(len, None);
			let mut buf = String::new();
			block_on(data.read_to_string(&mut buf)).unwrap();
			assert_eq!(buf, "async stream");
		}
		_ => panic!("Expected AsyncStream body"),
	}
}

#[test]
fn test_async_stream_fn_response() {
	let router = TestRouter::new();
	let executor = pin!(Executor::new());

	let headers = [];
	let req = Request::new("GET", "/asyncstreamfn", &headers, &[]);
	let mut res = block_on(router.route(req, executor.as_ref().spawner()));

	assert_eq!(res.status, 200);

	let body = res.body.take().unwrap();
	match body {
		Body::AsyncStream { mut data, len } => {
			assert_eq!(len, None);
			let mut buf = String::new();
			block_on(data.read_to_string(&mut buf)).unwrap();
			assert_eq!(buf, "async stream");
		}
		_ => panic!("Expected AsyncStream body"),
	}
}
