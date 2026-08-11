use crate::http::{Body, Request, Response};
use futures_lite::AsyncRead;
use std::io::{Cursor, Read};
use std::pin::Pin;

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

		let accept_encoding = req.find_header("Accept-Encoding").unwrap_or_default();

		if let Some(encoding) = parse_encodings(accept_encoding) {
			// Stream compression for all bodies
			resp.headers
				.retain(|n, _| !n.eq_ignore_ascii_case("content-length"));

			if matches!(resp.body, Some(Body::AsyncStream { .. })) {
				let data = match resp.body.take() {
					Some(Body::AsyncStream { data, .. }) => data,
					_ => unreachable!("Body is checked to be AsyncStream"),
				};

				let buf_reader = futures_lite::io::BufReader::new(data);

				use async_compression::{
					Level,
					futures::bufread::{BrotliEncoder, GzipEncoder, ZlibEncoder},
				};
				let compressed_stream: Pin<Box<dyn AsyncRead>> = match encoding {
					Encoding::Brotli => {
						Box::pin(BrotliEncoder::with_quality(buf_reader, Level::Precise(5)))
					}
					Encoding::Gzip => Box::pin(GzipEncoder::new(buf_reader)),
					Encoding::Deflate => Box::pin(ZlibEncoder::new(buf_reader)),
				};

				resp.body = Some(Body::AsyncStream {
					data: compressed_stream,
					len: None,
				});
			} else {
				let compressed_stream: Box<dyn Read + Send> = match encoding {
					Encoding::Brotli => match resp.body.take() {
						Some(Body::Immediate(data)) => Box::new(brotli::CompressorReader::new(
							Cursor::new(data),
							4 * 1024,
							5,
							20,
						)),
						Some(Body::Stream { data, .. }) => {
							Box::new(brotli::CompressorReader::new(data, 8 * 1024, 5, 20))
						}
						_ => unreachable!("Body exists and is not AsyncStream"),
					},
					Encoding::Gzip => match resp.body.take() {
						Some(Body::Immediate(data)) => Box::new(flate2::bufread::GzEncoder::new(
							Cursor::new(data),
							flate2::Compression::default(),
						)),
						Some(Body::Stream { data, .. }) => Box::new(flate2::read::GzEncoder::new(
							data,
							flate2::Compression::default(),
						)),
						_ => unreachable!("Body exists and is not AsyncStream"),
					},
					Encoding::Deflate => match resp.body.take() {
						Some(Body::Immediate(data)) => Box::new(flate2::bufread::ZlibEncoder::new(
							Cursor::new(data),
							flate2::Compression::default(),
						)),
						Some(Body::Stream { data, .. }) => Box::new(
							flate2::read::ZlibEncoder::new(data, flate2::Compression::default()),
						),
						_ => unreachable!("Body exists and is not AsyncStream"),
					},
				};

				resp.body = Some(Body::Stream {
					data: compressed_stream,
					len: None,
				});
			}

			resp.set_header("Content-Encoding", encoding.as_str());
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Encoding {
	Brotli,
	Gzip,
	Deflate,
}

impl Encoding {
	fn as_str(&self) -> &'static str {
		match self {
			Encoding::Brotli => "br",
			Encoding::Gzip => "gzip",
			Encoding::Deflate => "deflate",
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

fn parse_encodings(accept_encoding: &[u8]) -> Option<Encoding> {
	let mut q_br: Option<f32> = None;
	let mut q_gzip: Option<f32> = None;
	let mut q_deflate: Option<f32> = None;
	let mut q_identity: Option<f32> = None;
	let mut q_wildcard: Option<f32> = None;

	for entry in accept_encoding.split(|&b| b == b',') {
		let mut parts = entry.split(|&b| b == b';');
		let encoding = parts.next().unwrap_or(b"").trim_ascii();
		let mut qvalue = 1.0f32;

		for param in parts {
			let param = param.trim_ascii();
			if let Some(q_bytes) = param
				.strip_prefix(b"q=")
				.or_else(|| param.strip_prefix(b"Q="))
				&& let Ok(q_str) = std::str::from_utf8(q_bytes.trim_ascii())
				&& let Ok(q) = q_str.parse::<f32>()
				&& !q.is_nan()
			{
				qvalue = q;
			}
		}

		if encoding.eq_ignore_ascii_case(b"br") {
			q_br = Some(q_br.map_or(qvalue, |prev| prev.max(qvalue)));
		} else if encoding.eq_ignore_ascii_case(b"gzip") {
			q_gzip = Some(q_gzip.map_or(qvalue, |prev| prev.max(qvalue)));
		} else if encoding.eq_ignore_ascii_case(b"deflate") {
			q_deflate = Some(q_deflate.map_or(qvalue, |prev| prev.max(qvalue)));
		} else if encoding.eq_ignore_ascii_case(b"identity") {
			q_identity = Some(q_identity.map_or(qvalue, |prev| prev.max(qvalue)));
		} else if encoding == b"*" {
			q_wildcard = Some(q_wildcard.map_or(qvalue, |prev| prev.max(qvalue)));
		}
	}

	let q_br = q_br.or(q_wildcard).unwrap_or(0.0);
	let q_gzip = q_gzip.or(q_wildcard).unwrap_or(0.0);
	let q_deflate = q_deflate.or(q_wildcard).unwrap_or(0.0);
	let q_identity = q_identity.or(q_wildcard).unwrap_or(0.0);

	if q_br > 0.0 && q_br >= q_gzip && q_br >= q_deflate && q_br >= q_identity {
		Some(Encoding::Brotli)
	} else if q_gzip > 0.0 && q_gzip >= q_deflate && q_gzip >= q_identity {
		Some(Encoding::Gzip)
	} else if q_deflate > 0.0 && q_deflate >= q_identity {
		Some(Encoding::Deflate)
	} else {
		None
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::server::Server;
	use crate::server::handle_socket;
	use crate::server::task::{Executor, Spawner};
	use futures_lite::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
	use piper::{Reader, Writer};
	use std::io::Read;
	use std::pin::{Pin, pin};
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
		use_async_stream: bool,
	}

	impl Server for MockServer {
		async fn route<'e: 'r, 'r>(
			&'e self,
			_req: Request<'r, 'r>,
			_spawner: Spawner<'e>,
		) -> Response<'r> {
			let body = if self.use_async_stream {
				let (reader, mut writer) = piper::pipe(self.body.len() + 1);
				let body_bytes = self.body.clone();
				std::thread::spawn(move || {
					futures_lite::future::block_on(async move {
						let _ = writer.write_all(&body_bytes).await;
					});
				});
				Body::from_async_read(reader, None)
			} else if self.use_stream {
				Body::Stream {
					data: Box::new(Cursor::new(self.body.clone())),
					len: Some(self.body.len() as u64),
				}
			} else {
				Body::Immediate(self.body.clone())
			};

			Response::ok().with_body(body, Some(self.content_type.clone()))
		}
	}

	async fn run_test(server: MockServer, accept_encoding: Option<&str>) -> (String, Vec<u8>) {
		let (reader, mut client_tx) = piper::pipe(65536);
		let (mut client_rx, writer) = piper::pipe(65536);
		let socket = MockStream { reader, writer };
		let executor = pin!(Executor::new());
		let handle_future = handle_socket(socket, &server, executor.as_ref().spawner());

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
			use_async_stream: false,
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
			use_async_stream: false,
		};

		let (head, resp_body) = futures_lite::future::block_on(run_test(server, Some("br")));

		assert!(head.contains("Content-Encoding: br"));
		assert!(head.contains("Transfer-Encoding: chunked"));

		let chunk_decoded = decode_chunked(&resp_body);
		let decoded = decode_brotli(&chunk_decoded);
		assert_eq!(decoded, body);
	}

	#[test]
	fn test_compress_small_deflate_chunked() {
		let body = b"hello world".to_vec();
		let server = MockServer {
			body: body.clone(),
			content_type: "text/plain".to_string(),
			use_stream: false,
			use_async_stream: false,
		};

		let (head, resp_body) = futures_lite::future::block_on(run_test(server, Some("deflate")));

		assert!(head.contains("Content-Encoding: deflate"));
		assert!(head.contains("Transfer-Encoding: chunked"));

		let chunk_decoded = decode_chunked(&resp_body);
		let decoded = decode_zlib(&chunk_decoded);
		assert_eq!(decoded, body);
	}

	#[test]
	fn test_compress_async_stream_gzip() {
		let body = b"hello world from async stream gzip".to_vec();
		let server = MockServer {
			body: body.clone(),
			content_type: "text/plain".to_string(),
			use_stream: false,
			use_async_stream: true,
		};

		let (head, resp_body) = futures_lite::future::block_on(run_test(server, Some("gzip")));

		assert!(head.contains("Content-Encoding: gzip"));
		assert!(head.contains("Transfer-Encoding: chunked"));

		let chunk_decoded = decode_chunked(&resp_body);
		let decoded = decode_gzip(&chunk_decoded);
		assert_eq!(decoded, body);
	}

	#[test]
	fn test_compress_async_stream_brotli() {
		let body = b"hello world from async stream brotli".to_vec();
		let server = MockServer {
			body: body.clone(),
			content_type: "text/plain".to_string(),
			use_stream: false,
			use_async_stream: true,
		};

		let (head, resp_body) = futures_lite::future::block_on(run_test(server, Some("br")));

		assert!(head.contains("Content-Encoding: br"));
		assert!(head.contains("Transfer-Encoding: chunked"));

		let chunk_decoded = decode_chunked(&resp_body);
		let decoded = decode_brotli(&chunk_decoded);
		assert_eq!(decoded, body);
	}

	#[test]
	fn test_compress_async_stream_deflate() {
		let body = b"hello world from async stream deflate".to_vec();
		let server = MockServer {
			body: body.clone(),
			content_type: "text/plain".to_string(),
			use_stream: false,
			use_async_stream: true,
		};

		let (head, resp_body) = futures_lite::future::block_on(run_test(server, Some("deflate")));

		assert!(head.contains("Content-Encoding: deflate"));
		assert!(head.contains("Transfer-Encoding: chunked"));

		let chunk_decoded = decode_chunked(&resp_body);
		let decoded = decode_zlib(&chunk_decoded);
		assert_eq!(decoded, body);
	}

	#[test]
	fn test_compress_accept_encoding_quality_zero() {
		let body = b"hello uncompressed because q=0".to_vec();
		let server = MockServer {
			body: body.clone(),
			content_type: "text/plain".to_string(),
			use_stream: false,
			use_async_stream: false,
		};

		let (head, resp_body) =
			futures_lite::future::block_on(run_test(server, Some("gzip;q=0, br;q=0")));

		assert!(!head.contains("Content-Encoding:"));
		assert_eq!(resp_body, body);
	}

	#[test]
	fn test_parse_encodings_unit() {
		// Defaults & implicit q=1.0
		assert_eq!(parse_encodings(b"gzip"), Some(Encoding::Gzip));
		assert_eq!(parse_encodings(b"br"), Some(Encoding::Brotli));
		assert_eq!(parse_encodings(b"deflate"), Some(Encoding::Deflate));
		assert_eq!(parse_encodings(b"*"), Some(Encoding::Brotli));
		assert_eq!(parse_encodings(b"gzip, br"), Some(Encoding::Brotli));
		assert_eq!(parse_encodings(b"deflate, gzip"), Some(Encoding::Gzip));

		// Explicit q-value weighting
		assert_eq!(
			parse_encodings(b"gzip;q=0.9, br;q=0.5"),
			Some(Encoding::Gzip)
		);
		assert_eq!(
			parse_encodings(b"deflate;q=1.0, gzip;q=0.5, br;q=0.1"),
			Some(Encoding::Deflate)
		);
		assert_eq!(
			parse_encodings(b"br;q=0.8, gzip;q=0.8"),
			Some(Encoding::Brotli)
		);
		assert_eq!(
			parse_encodings(b"gzip;q=0.8, deflate;q=0.8"),
			Some(Encoding::Gzip)
		);

		// Wildcards
		assert_eq!(parse_encodings(b"*;q=0"), None);
		assert_eq!(
			parse_encodings(b"*;q=0.5, gzip;q=0.8"),
			Some(Encoding::Gzip)
		);
		assert_eq!(
			parse_encodings(b"*;q=0.8, gzip;q=0.2"),
			Some(Encoding::Brotli)
		);

		// q=0 / exclusion
		assert_eq!(parse_encodings(b"gzip;q=0, br;q=0, deflate;q=0"), None);
		assert_eq!(
			parse_encodings(b"gzip;q=0, br;q=0.8"),
			Some(Encoding::Brotli)
		);
		assert_eq!(parse_encodings(b""), None);

		// Identity handling
		assert_eq!(parse_encodings(b"identity"), None);
		assert_eq!(parse_encodings(b"identity;q=1.0, gzip;q=0.5"), None);
		assert_eq!(
			parse_encodings(b"identity;q=0.5, gzip;q=0.8"),
			Some(Encoding::Gzip)
		);
		assert_eq!(
			parse_encodings(b"gzip;q=1.0, identity;q=1.0"),
			Some(Encoding::Gzip)
		);
	}
}
