use moonbeam::{Request, Response, Spawner, ThreadCount, serve_multi, server, assets::get_asset};

#[server(StaticServer)]
async fn serve(req: Request, _spawner: Spawner) -> Response {
	let etag = req.find_header("If-None-Match");
	get_asset(req.path, etag, "benchmarks/static").await
}

pub fn main() {
	println!("Moonbeam MT (Static) listening on http://127.0.0.1:3030/");
	serve_multi(
		"127.0.0.1:3030",
		ThreadCount::Count(4),
		|| StaticServer,
	);
}
