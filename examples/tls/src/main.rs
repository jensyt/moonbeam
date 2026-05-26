use moonbeam::{Body, Request, Response, Spawner, TlsConfig, server};
use std::env;
use std::sync::atomic::{AtomicUsize, Ordering};

struct State {
	thread_id: Option<usize>,
}

#[server(HelloWorld)]
async fn serve(_request: Request, _spawner: Spawner, state: &State) -> Response {
	let msg = match state.thread_id {
		Some(id) => format!("Hello from secure thread {}!", id),
		None => "Hello, Secure World!".to_string(),
	};
	Response::new_with_body(msg, Body::TEXT)
}

fn main() {
	let args: Vec<String> = env::args().collect();
	let multi_threaded = args.contains(&"--multi".to_string());

	// Generate a self-signed certificate for local testing
	println!("Generating self-signed certificate...");
	let subject_alt_names = vec!["127.0.0.1".to_string(), "localhost".to_string()];
	let cert = rcgen::generate_simple_self_signed(subject_alt_names)
		.expect("Failed to generate self-signed certificate");

	let cert_der = cert.cert.der().to_vec();
	let key_der = cert.signing_key.serialize_der();

	let server_config = TlsConfig::from_raw(vec![cert_der], key_der)
		.into_server_config()
		.expect("Invalid TLS config");

	let addr = "127.0.0.1:7463";
	println!("Running TLS server on https://{addr}. Press Ctrl+C to exit");
	println!(
		"Note: Since this uses a self-signed certificate, \
		you will need to accept the security warning in your browser \
		or use `curl -k`."
	);

	if multi_threaded {
		println!("Mode: Multi-threaded (share-nothing isolation)");
		let next_id = AtomicUsize::new(0);
		moonbeam::serve_multi_tls(addr, moonbeam::ThreadCount::Default, server_config, || {
			let id = next_id.fetch_add(1, Ordering::Relaxed);
			HelloWorld(State {
				thread_id: Some(id),
			})
		});
	} else {
		println!("Mode: Single-threaded (run with --multi for multi-threaded mode)");
		moonbeam::serve_tls(addr, server_config, || {
			HelloWorld(State { thread_id: None })
		});
	}
}
