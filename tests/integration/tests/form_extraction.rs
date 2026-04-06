use futures_lite::future::block_on;
use moonbeam::{Body, Header, Request, Response, Server, route, router};
use moonbeam_serde::{File, Form};
use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq)]
struct User<'a> {
	id: u32,
	#[serde(borrow)]
	name: &'a str,
	active: bool,
}

#[route]
async fn handle_form(Form(user): Form<User<'_>>) -> Response {
	Response::ok().with_body(
		format!("{}:{}:{}", user.id, user.name, user.active),
		Body::TEXT,
	)
}

#[derive(Debug, Deserialize)]
struct Upload<'a> {
	title: &'a str,
	file: File<'a>,
}

#[route]
async fn handle_upload(Form(u): Form<Upload<'_>>) -> Response {
	Response::ok().with_body(
		format!(
			"{}:{}:{}",
			u.title,
			u.file.name.unwrap_or(""),
			u.file.data.len()
		),
		Body::TEXT,
	)
}

router!(MyRouter {
	post("/submit") => handle_form,
	post("/upload") => handle_upload,
});

#[test]
fn test_integration_form_urlencoded() {
	let router = Box::leak(Box::new(MyRouter::new()));
	let body = b"id=42&name=Jens&active=true";
	let headers = [
		Header {
			name: "Content-Type",
			value: b"application/x-www-form-urlencoded",
		},
		Header {
			name: "Content-Length",
			value: b"27",
		},
	];
	let req = Request::new("POST", "/submit", &headers, body);
	let res = block_on(router.route(req));
	assert_eq!(res.status, 200);
	assert_body(res.body, "42:Jens:true");
}

#[test]
fn test_integration_form_multipart() {
	let router = Box::leak(Box::new(MyRouter::new()));
	let body = b"--boundary\r\n\
				Content-Disposition: form-data; name=\"id\"\r\n\
				\r\n\
				42\r\n\
				--boundary\r\n\
				Content-Disposition: form-data; name=\"name\"\r\n\
				\r\n\
				Jens\r\n\
				--boundary\r\n\
				Content-Disposition: form-data; name=\"active\"\r\n\
				\r\n\
				yes\r\n\
				--boundary--";

	let cl = body.len().to_string();
	let headers = [
		Header {
			name: "Content-Type",
			value: b"multipart/form-data; boundary=boundary",
		},
		Header {
			name: "Content-Length",
			value: cl.as_bytes(),
		},
	];
	let req = Request::new("POST", "/submit", &headers, body);
	let res = block_on(router.route(req));
	assert_eq!(res.status, 200);
	assert_body(res.body, "42:Jens:true");
}

#[test]
fn test_integration_form_file_upload() {
	let router = Box::leak(Box::new(MyRouter::new()));
	let body = b"--boundary\r\n\
				Content-Disposition: form-data; name=\"title\"\r\n\
				\r\n\
				My File\r\n\
				--boundary\r\n\
				Content-Disposition: form-data; name=\"file\"; filename=\"test.txt\"\r\n\
				Content-Type: text/plain\r\n\
				\r\n\
				Hello World\r\n\
				--boundary--";

	let cl = body.len().to_string();
	let headers = [
		Header {
			name: "Content-Type",
			value: b"multipart/form-data; boundary=boundary",
		},
		Header {
			name: "Content-Length",
			value: cl.as_bytes(),
		},
	];
	let req = Request::new("POST", "/upload", &headers, body);
	let res = block_on(router.route(req));
	assert_eq!(res.status, 200);
	assert_body(res.body, "My File:test.txt:11");
}

// Helper to check body content
fn assert_body(body: Option<Body>, expected: &str) {
	match body {
		Some(Body::Immediate(data)) => {
			assert_eq!(String::from_utf8_lossy(&data), expected);
		}
		_ => panic!("Expected immediate body"),
	}
}
