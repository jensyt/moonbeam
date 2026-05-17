use axum::Router;
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() {
    let app = Router::new().fallback_service(ServeDir::new("benchmarks/static"));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3030").await.unwrap();
    println!("Tokio (Axum Static) listening on http://127.0.0.1:3030/");
    axum::serve(listener, app).await.unwrap();
}
