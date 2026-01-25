use crate::http::{Body, Request, Response};
use std::io::{Cursor, Read};

pub fn apply_compression(req: &Request, resp: &mut Response) {
	if resp.body.is_none() && resp.status != 304 {
		return;
	}

	let already_compressed = resp
		.headers
		.iter()
		.any(|(n, _)| n.eq_ignore_ascii_case("content-encoding"));

	let compressible_type = resp
		.headers
		.iter()
		.find(|(n, _)| n.eq_ignore_ascii_case("content-type"))
		.map(|(_, v)| is_compressible(v))
		.unwrap_or(false);

	if !already_compressed && compressible_type {
		if !resp.headers.iter().any(|(n, v)| {
			n.eq_ignore_ascii_case("vary") && v.eq_ignore_ascii_case("Accept-Encoding")
		}) {
			resp.headers.push("Vary", "Accept-Encoding");
		}

		if resp.status == 304 {
			return;
		}

		let accept_encoding = req
			.find_header("Accept-Encoding")
			.map(|v| String::from_utf8_lossy(v).to_string())
			.unwrap_or_default();

		let use_brotli = accept_encoding.contains("br");
		let use_gzip = accept_encoding.contains("gzip");
		let use_deflate = accept_encoding.contains("deflate");

		if use_brotli || use_gzip || use_deflate {
			// Stream compression for all bodies
			resp.headers
				.retain(|n, _| !n.eq_ignore_ascii_case("content-length"));

			let compressed_stream: Box<dyn Read + Send + 'static> = if use_brotli {
				match resp.body.take() {
					Some(Body::Immediate(data)) => Box::new(brotli::CompressorReader::new(
						Cursor::new(data),
						4 * 1024,
						5,
						20,
					)),
					Some(Body::Stream { data, .. }) => {
						Box::new(brotli::CompressorReader::new(data, 8 * 1024, 5, 20))
					}
					None => {
						unreachable!(
							"Compression applied to empty body (checked at function start)"
						)
					}
				}
			} else if use_gzip {
				match resp.body.take() {
					Some(Body::Immediate(data)) => Box::new(flate2::bufread::GzEncoder::new(
						Cursor::new(data),
						flate2::Compression::default(),
					)),
					Some(Body::Stream { data, .. }) => Box::new(flate2::read::GzEncoder::new(
						data,
						flate2::Compression::default(),
					)),
					None => {
						unreachable!(
							"Compression applied to empty body (checked at function start)"
						)
					}
				}
			} else {
				match resp.body.take() {
					Some(Body::Immediate(data)) => Box::new(flate2::bufread::ZlibEncoder::new(
						Cursor::new(data),
						flate2::Compression::default(),
					)),
					Some(Body::Stream { data, .. }) => Box::new(flate2::read::ZlibEncoder::new(
						data,
						flate2::Compression::default(),
					)),
					None => {
						unreachable!(
							"Compression applied to empty body (checked at function start)"
						)
					}
				}
			};

			resp.body = Some(Body::Stream {
				data: compressed_stream,
				len: None,
			});

			let encoding = if use_brotli {
				"br"
			} else if use_gzip {
				"gzip"
			} else {
				"deflate"
			};
			resp.set_header("Content-Encoding", encoding);
		}
	}
}

fn is_compressible(content_type: &str) -> bool {
	let ct = content_type.trim().to_ascii_lowercase();
	ct.starts_with("text/")
		|| ct.starts_with("application/json")
		|| ct.starts_with("application/xml")
		|| ct.starts_with("application/javascript")
		|| ct.starts_with("application/xhtml+xml")
		|| ct.starts_with("image/svg+xml")
		|| ct.starts_with("application/rss+xml")
		|| ct.starts_with("application/atom+xml")
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::server::Server;
	use crate::server::handle_socket;
	use futures_lite::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
	use piper::{Reader, Writer};
	use std::io::Read;
	use std::pin::Pin;
	use std::task::{Context, Poll};

	struct MockStream {
		reader: Reader,
		writer: Writer,
	}

	impl AsyncRead for MockStream {
		fn poll_read(
			mut self: Pin<&mut Self>,
			cx: &mut Context<'_>,
			buf: &mut [u8],
		) -> Poll<std::io::Result<usize>> {
			Pin::new(&mut self.reader).poll_read(cx, buf)
		}
	}

	impl AsyncWrite for MockStream {
		fn poll_write(
			mut self: Pin<&mut Self>,
			cx: &mut Context<'_>,
			buf: &[u8],
		) -> Poll<std::io::Result<usize>> {
			Pin::new(&mut self.writer).poll_write(cx, buf)
		}

		fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
			Pin::new(&mut self.writer).poll_flush(cx)
		}

		fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
			Pin::new(&mut self.writer).poll_close(cx)
		}
	}

	struct MockServer {
		body: Vec<u8>,
		content_type: String,
		use_stream: bool,
	}

	impl Server for MockServer {
		async fn route(&'static self, _req: Request<'_, '_>) -> Response {
			let body = if self.use_stream {
				Body::Stream {
					data: Box::new(Cursor::new(self.body.clone())),
					len: Some(self.body.len() as u64),
				}
			} else {
				Body::Immediate(self.body.clone())
			};

			Response::ok().with_body(body, Some(&self.content_type))
		}
	}

	async fn run_test(server: MockServer, accept_encoding: Option<&str>) -> (String, Vec<u8>) {
		let (reader, mut client_tx) = piper::pipe(65536);
		let (mut client_rx, writer) = piper::pipe(65536);
		let socket = MockStream { reader, writer };
		let server = Box::leak(Box::new(server));

		let handle_future = handle_socket(socket, "127.0.0.1:80".parse().unwrap(), server);

		let test_future = async move {
			let mut headers = "GET / HTTP/1.1\r\n".to_string();
			if let Some(enc) = accept_encoding {
				headers.push_str(&format!("Accept-Encoding: {}\r\n", enc));
			}
			headers.push_str("\r\n");

			client_tx.write_all(headers.as_bytes()).await.unwrap();
			client_tx.close().await.unwrap();

			let mut buf = Vec::new();
			client_rx.read_to_end(&mut buf).await.unwrap();
			buf
		};

		let (_, buf) = futures_lite::future::zip(handle_future, test_future).await;

		let mut headers = [httparse::EMPTY_HEADER; 32];
		let mut resp = httparse::Response::new(&mut headers);
		let status = resp.parse(&buf).unwrap();

		let body_start = status.unwrap();
		let head_str = String::from_utf8_lossy(&buf[..body_start]).to_string();
		let body_bytes = buf[body_start..].to_vec();

		(head_str, body_bytes)
	}

	fn decode_gzip(data: &[u8]) -> Vec<u8> {
		let mut d = flate2::read::GzDecoder::new(data);
		let mut s = Vec::new();
		d.read_to_end(&mut s).unwrap();
		s
	}

	fn decode_brotli(data: &[u8]) -> Vec<u8> {
		let mut d = brotli::Decompressor::new(data, 4096);
		let mut s = Vec::new();
		d.read_to_end(&mut s).unwrap();
		s
	}

	fn decode_zlib(data: &[u8]) -> Vec<u8> {
		let mut d = flate2::read::ZlibDecoder::new(data);
		let mut s = Vec::new();
		d.read_to_end(&mut s).unwrap();
		s
	}

	fn decode_chunked(data: &[u8]) -> Vec<u8> {
		let mut res = Vec::new();
		let mut cur = std::io::Cursor::new(data);
		loop {
			let mut line = String::new();
			let mut char_buf = [0u8; 1];
			loop {
				if cur.read(&mut char_buf).unwrap() == 0 {
					return res;
				}
				let c = char_buf[0] as char;
				line.push(c);
				if line.ends_with("\r\n") {
					break;
				}
			}
			let len_str = line.trim();
			if len_str.is_empty() {
				continue;
			}
			let len = usize::from_str_radix(len_str, 16).unwrap();
			if len == 0 {
				break;
			}

			let mut chunk = vec![0u8; len];
			cur.read_exact(&mut chunk).unwrap();
			res.extend_from_slice(&chunk);

			let mut dump = [0u8; 2];
			cur.read_exact(&mut dump).unwrap();
		}
		res
	}

	#[test]
	fn test_compress_small_gzip_chunked() {
		// Even small bodies should be chunked now
		let body = b"hello world".to_vec();
		let server = MockServer {
			body: body.clone(),
			content_type: "text/plain".to_string(),
			use_stream: false,
		};

		let (head, resp_body) = futures_lite::future::block_on(run_test(server, Some("gzip")));

		assert!(head.contains("Content-Encoding: gzip"));
		assert!(head.contains("Transfer-Encoding: chunked"));
		assert!(!head.contains("Content-Length"));

		let chunk_decoded = decode_chunked(&resp_body);
		let decoded = decode_gzip(&chunk_decoded);
		assert_eq!(decoded, body);
	}

	#[test]
	fn test_compress_small_brotli_chunked() {
		let body = b"hello world".to_vec();
		let server = MockServer {
			body: body.clone(),
			content_type: "text/plain".to_string(),
			use_stream: false,
		};

		let (head, resp_body) = futures_lite::future::block_on(run_test(server, Some("br")));

		assert!(head.contains("Content-Encoding: br"));
		assert!(head.contains("Transfer-Encoding: chunked"));

		let chunk_decoded = decode_chunked(&resp_body);
		let decoded = decode_brotli(&chunk_decoded);
		assert_eq!(decoded, body);
	}

	#[test]
	fn test_preference_br_over_gzip() {
		let body = b"hello".to_vec();
		let server = MockServer {
			body: body.clone(),
			content_type: "text/plain".to_string(),
			use_stream: false,
		};

		let (head, _) = futures_lite::future::block_on(run_test(server, Some("gzip, br")));
		assert!(head.contains("Content-Encoding: br"));
	}

	#[test]
	fn test_compress_small_deflate_chunked() {
		let body = b"hello world".to_vec();
		let server = MockServer {
			body: body.clone(),
			content_type: "text/plain".to_string(),
			use_stream: false,
		};

		let (head, resp_body) = futures_lite::future::block_on(run_test(server, Some("deflate")));

		assert!(head.contains("Content-Encoding: deflate"));
		assert!(head.contains("Transfer-Encoding: chunked"));

		let chunk_decoded = decode_chunked(&resp_body);
		let decoded = decode_zlib(&chunk_decoded);
		assert_eq!(decoded, body);
	}

	#[test]
	fn test_preference_gzip_over_deflate() {
		let body = b"hello".to_vec();
		let server = MockServer {
			body: body.clone(),
			content_type: "text/plain".to_string(),
			use_stream: false,
		};

		let (head, _) = futures_lite::future::block_on(run_test(server, Some("deflate, gzip")));
		assert!(head.contains("Content-Encoding: gzip"));
	}
}
