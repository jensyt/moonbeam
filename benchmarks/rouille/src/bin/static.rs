fn main() {
	println!("Rouille Static server listening on http://127.0.0.1:3030/");

	rouille::start_server("127.0.0.1:3030", move |request| {
		rouille::match_assets(request, "benchmarks/static")
	});
}
