use async_io::Timer;
use moonbeam::{AsyncStreamWriter, Body, Request, Response, Spawner, SseEvent, serve, server};
use moonbeam_serde::WithJsonData;
use serde::Serialize;
use std::{cell::Cell, time::Duration};

struct State {
	count: Cell<usize>,
}

#[derive(Serialize)]
struct Message<'a> {
	msg: &'a str,
}

async fn sse(writer: AsyncStreamWriter, req: Request<'_, '_>, state: &State) {
	for _ in 0..10 {
		writer
			.write_string(
				SseEvent::new()
					.with_json_data(Message {
						msg: &format!("hello {} from {}", state.count.get(), req.path),
					})
					.with_event("ping"),
			)
			.await;
		state.count.update(|v| v + 1);
		Timer::after(Duration::from_secs(1)).await;
	}
	writer
		.write_string(SseEvent::new().with_event("close").with_data(""))
		.await;
}

#[server(SseServer)]
async fn handle_request(req: Request, _spawner: Spawner, state: &State) -> Response {
	if req.path == "/events" {
		Response::new_from_sse_fn(|writer| sse(writer, req, state))
	} else {
		let html = r#"
		<!DOCTYPE html>
		<html>
		<head>
			<title>Moonbeam SSE Example</title>
		</head>
		<body>
			<h1>Server-Sent Events</h1>
			<ul id="events"></ul>
			<script>
				const evtSource = new EventSource("/events");
				evtSource.addEventListener("ping", (e) => {
					const newElement = document.createElement("li");
					const data = JSON.parse(e.data);
					newElement.textContent = `Ping received: ${data.msg} at ${new Date().toLocaleTimeString()}`;
					document.getElementById("events").appendChild(newElement);
				});
				evtSource.addEventListener("close", (e) => {
					const newElement = document.createElement("li");
					newElement.textContent = "Connection closed by server.";
					document.getElementById("events").appendChild(newElement);
					evtSource.close();
				});
			</script>
		</body>
		</html>
		"#;
		Response::new_with_body(html, Body::HTML)
	}
}

fn main() {
	println!("Starting SSE server on http://127.0.0.1:8080");
	serve("127.0.0.1:8080", || {
		SseServer(State {
			count: Cell::new(0),
		})
	})
}
