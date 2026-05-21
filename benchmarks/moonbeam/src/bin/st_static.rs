use moonbeam::{Request, Response, Spawner, server, assets::get_asset};

#[server(StaticServer)]
async fn serve(req: Request, _spawner: Spawner) -> Response {
	let etag = req.find_header("If-None-Match");
	get_asset(req.path, etag, "benchmarks/static").await
}

fn main() {
	println!("Moonbeam ST (Static) listening on http://127.0.0.1:3030/");
	moonbeam::serve("127.0.0.1:3030", StaticServer);
}
