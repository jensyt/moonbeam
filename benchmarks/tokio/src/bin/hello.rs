use axum::{routing::get, Router};

#[tokio::main]
async fn main() {
    let app = Router::new().route("/", get(|| async { "Hello, World!" }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3030").await.unwrap();
    println!("Tokio (Axum) listening on http://127.0.0.1:3030/");
    axum::serve(listener, app).await.unwrap();
}
