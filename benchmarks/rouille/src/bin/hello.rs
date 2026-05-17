use rouille::Response;

fn main() {
	println!("Rouille server listening on http://127.0.0.1:3030/");

	rouille::start_server("127.0.0.1:3030", move |_request| {
		Response::text("Hello, World!")
	});
}
