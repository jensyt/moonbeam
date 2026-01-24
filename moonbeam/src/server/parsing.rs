use crate::http::Request;
use httparse::{Header, Request as RawRequest, Status};
use std::mem::MaybeUninit;

/// Scan for the `\r\n\r\n` at the end of an HTTP header.
///
/// If the end is found, returns `Some(offset)` to the *end* of the header, i.e. after `\r\n\r\n`.
/// If the end is not found, returns `None`.
///
/// # Examples
/// ```ignore
/// let buffer = b"GET /file HTTP/1.1\r\nHost: example.com\r\n\r\n";
/// assert_eq!(scan_for_header_end(buffer), Some(41));
/// ```
#[cfg(all(
	target_arch = "aarch64",
	target_feature = "neon",
	not(feature = "disable-simd")
))]
fn scan_for_header_end_simd(buffer: &[u8]) -> Option<usize> {
	use core::arch::aarch64::*;

	let len = buffer.len();
	let mut offset = 0;

	unsafe {
		let cr = vdupq_n_u8(b'\r');
		let lf = vdupq_n_u8(b'\n');
		let ptr = buffer.as_ptr();
		let len_16 = len.saturating_sub(16);
		while offset <= len_16 {
			// Read blocks of 16 chars
			let block = vld1q_u8(ptr.add(offset));
			// Check for \r or \n
			let mask = vorrq_u8(vceqq_u8(block, cr), vceqq_u8(block, lf));
			// Determine how many \r and \n there are. Each match adds 255.
			let count = vaddlvq_u8(mask);
			if count == 0 {
				// None means we can skip this whole block
				offset += 16;
			} else if count < 1020 {
				// Less than 4x255 means '\r\n\r\n' definitely doesn't appear in the first half of
				// this block, but we can't be certain it isn't in the second half
				offset += 8;
			} else {
				// At least 4x255 means '\r\n\r\n' could be somewhere in this block, so we need to
				// scan sequentially
				let (mut eq, mut mask) = if cfg!(target_endian = "little") {
					let eq = vcombine_u8(
						vcreate_u8(0x000000000A0D0A0D), // xxxx\n\r\n\r
						vcreate_u8(0x0000000000000000), // xxxxxxxx
					);
					let mask = vcombine_u8(
						vcreate_u8(0xFFFFFFFF00000000), // 11110000
						vcreate_u8(0xFFFFFFFFFFFFFFFF), // 11111111
					);
					(eq, mask)
				} else {
					let eq = vcombine_u8(
						vcreate_u8(0x0D0A0D0A00000000), // \r\n\r\nxxxx
						vcreate_u8(0x0000000000000000), // xxxxxxxx
					);
					let mask = vcombine_u8(
						vcreate_u8(0x00000000FFFFFFFF), // 00001111
						vcreate_u8(0xFFFFFFFFFFFFFFFF), // 11111111
					);
					(eq, mask)
				};

				// Check all positions \r\n\r\nXXXXXXXXXXXX to XXXXXXXXXXXX\r\n\r\n
				for i in 0..=12 {
					// Pass if we found \r\n\r\n in the desired position and ignoring every other
					// byte (or with 0xFF)
					let res = vorrq_u8(vceqq_u8(block, eq), mask);
					// If we found a match all bytes should be 0xFF
					let pass = vminvq_u8(res);
					if pass == 0 {
						// Rotate right 1 position
						eq = vextq_u8::<15>(eq, eq);
						mask = vextq_u8::<15>(mask, mask);
					} else {
						// Add four to account for the \r\n\r\n itself
						return Some(offset + i + 4);
					}
				}

				// Not found, jump ahead by a half block
				offset += 8;
			}
		}
	}

	if offset < len {
		scan_for_header_end_simple(&buffer[offset..]).map(|v| v + offset)
	} else {
		None
	}
}

#[cfg(all(
	target_arch = "x86_64",
	target_feature = "sse2",
	not(target_feature = "sse4.2"),
	not(feature = "disable-simd")
))]
fn scan_for_header_end_simd(buffer: &[u8]) -> Option<usize> {
	use core::arch::x86_64::*;

	let len = buffer.len();
	let mut offset = 0;

	unsafe {
		let cr = _mm_set1_epi8(b'\r' as i8);
		let lf = _mm_set1_epi8(b'\n' as i8);
		let ptr = buffer.as_ptr();
		let len_16 = len.saturating_sub(16);

		while offset <= len_16 {
			let block = _mm_loadu_si128(ptr.add(offset) as *const __m128i);

			// Create bitmasks of \r and \n occurrences
			let cr_mask = _mm_movemask_epi8(_mm_cmpeq_epi8(block, cr));
			let lf_mask = _mm_movemask_epi8(_mm_cmpeq_epi8(block, lf));

			// Find where \r is followed by \n
			let rn_mask = (cr_mask as u32) & ((lf_mask as u32) >> 1);
			// Find where \r\n is followed by \r\n two bytes later
			let rnrn_mask = rn_mask & (rn_mask >> 2);

			if rnrn_mask != 0 {
				// Find the index of the first match
				let match_idx = rnrn_mask.trailing_zeros();
				return Some(offset + match_idx as usize + 4);
			}

			// If no match, determine how far to jump.
			// If there are no \r or \n characters, we can safely skip the whole block.
			if (cr_mask | lf_mask) == 0 {
				offset += 16;
			} else {
				// Otherwise, a jump of 13 is safe, as a 4-byte pattern can't be missed.
				offset += 13;
			}
		}
	}

	// Fallback for the remaining part of the buffer
	if offset < len {
		scan_for_header_end_simple(&buffer[offset..]).map(|v| v + offset)
	} else {
		None
	}
}

/// This is a hypothetical implementation for demonstration.
/// It requires the `sse4.2` target feature.
#[cfg(all(
	target_arch = "x86_64",
	target_feature = "sse4.2",
	not(feature = "disable-simd")
))]
fn scan_for_header_end_simd(buffer: &[u8]) -> Option<usize> {
	use core::arch::x86_64::*;

	const MODE: i32 = _SIDD_CMP_EQUAL_ORDERED | _SIDD_UBYTE_OPS;

	let len = buffer.len();
	let mut offset = 0;
	let ptr = buffer.as_ptr();

	// The pattern to search for: \r\n\r\n
	let pattern =
		unsafe { _mm_loadu_si128(b"\r\n\r\n\0\0\0\0\0\0\0\0\0\0\0\0".as_ptr() as *const _) };

	while offset + 16 <= len {
		let block = unsafe { _mm_loadu_si128(ptr.add(offset) as *const _) };

		// Search for the 4-byte pattern in the 16-byte block.
		let index = unsafe { _mm_cmpistri(pattern, block, MODE) };

		if index < 16 {
			// A match was found at `index` within the block.
			return Some(offset + index as usize + 4);
		}

		// If no match, jump ahead. A jump of 13 is safe for a 4-byte pattern.
		offset += 13;
	}

	// Fallback for the remainder of the buffer.
	if offset < len {
		scan_for_header_end_simple(&buffer[offset..]).map(|v| v + offset)
	} else {
		None
	}
}

fn scan_for_header_end_simple(buffer: &[u8]) -> Option<usize> {
	let sequence = b"\r\n\r\n";
	let slen = sequence.len();

	for i in 0..buffer.len().saturating_sub(slen - 1) {
		if &buffer[i..i + slen] == sequence {
			return Some(i + slen);
		}
	}

	None
}

pub(super) fn scan_for_header_end(buffer: &[u8]) -> Option<usize> {
	#[cfg(all(
		any(
			all(target_arch = "aarch64", target_feature = "neon"),
			all(target_arch = "x86_64", target_feature = "sse2")
		),
		not(feature = "disable-simd")
	))]
	{
		scan_for_header_end_simd(buffer)
	}

	#[cfg(any(
		not(any(
			all(target_arch = "aarch64", target_feature = "neon"),
			all(target_arch = "x86_64", target_feature = "sse2")
		)),
		feature = "disable-simd"
	))]
	{
		scan_for_header_end_simple(buffer)
	}
}

pub(super) enum ParseError {
	NoEndFound,
	HTTPParseError(httparse::Error),
}

impl std::fmt::Debug for ParseError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::NoEndFound => write!(f, "no end found"),
			Self::HTTPParseError(e) => write!(f, "{e:?}"),
		}
	}
}

pub(super) fn parse_http_request<'buf, 'header>(
	buffer: &'buf [u8],
	headers: &'header mut [MaybeUninit<Header<'buf>>],
) -> Result<Request<'header, 'buf>, ParseError> {
	let mut req = RawRequest::new(&mut []);
	match req.parse_with_uninit_headers(buffer, headers) {
		Ok(Status::Partial) => Err(ParseError::NoEndFound),
		Err(e) => Err(ParseError::HTTPParseError(e)),
		_ => Ok(Request::new_from_raw(req, &buffer[0..0])),
	}
}

pub(super) fn get_important_headers(request: &Request) -> (usize, bool) {
	let mut contentlength = 0;
	let mut close = false;
	for &Header { name, value } in request.headers.iter() {
		if name.eq_ignore_ascii_case("content-length") {
			contentlength = std::str::from_utf8(value)
				.ok()
				.and_then(|v| v.parse().ok())
				.unwrap_or(0);
		} else if name.eq_ignore_ascii_case("connection") {
			close = std::str::from_utf8(value)
				.ok()
				.is_some_and(|v| v.eq_ignore_ascii_case("close"));
		}
	}
	(contentlength, close)
}

// pub(super) fn get_contentlength(request: &Request) -> usize {
// 	request
// 		.headers
// 		.iter()
// 		.find(|h| h.name.eq_ignore_ascii_case("content-length"))
// 		.and_then(|h| std::str::from_utf8(h.value).ok())
// 		.and_then(|v| v.parse().ok())
// 		.unwrap_or(0)
// }

// pub(super) fn get_connectionclose(request: &Request) -> bool {
// 	request
// 		.headers
// 		.iter()
// 		.find(|h| h.name.eq_ignore_ascii_case("connection"))
// 		.and_then(|h| std::str::from_utf8(h.value).ok())
// 		.map_or(false, |v| v.eq_ignore_ascii_case("close"))
// }

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_header_scan() {
		let request = b"POST /users HTTP/1.1\r\nHost: example.com\r\n\
			Content-Type: application/x-www-form-urlencoded\r\nContent-Length: 49\r\n\r\n\
			name=FirstName+LastName&email=bsmth%40example.com";
		assert_eq!(scan_for_header_end(request), Some(112));

		let request = b"POST /users HTTP/1.1\r\n\n\nHost: example.com\r\n\
			Content-Type: application/x-www-form-urlencoded\r\nContent-Length: 49\r\n\r\n";
		assert_eq!(scan_for_header_end(request), Some(114));

		let request = b"GET /file HTTP/1.1\r\nHost: example.com\r\n\r\n";
		assert_eq!(scan_for_header_end(request), Some(41));

		assert_eq!(scan_for_header_end(b"\r\n\r\nXXXXXXXXXXXX"), Some(4));
		assert_eq!(scan_for_header_end(b"X\r\n\r\nXXXXXXXXXXX"), Some(5));
		assert_eq!(scan_for_header_end(b"XX\r\n\r\nXXXXXXXXXX"), Some(6));
		assert_eq!(scan_for_header_end(b"XXX\r\n\r\nXXXXXXXXX"), Some(7));
		assert_eq!(scan_for_header_end(b"XXXX\r\n\r\nXXXXXXXX"), Some(8));
		assert_eq!(scan_for_header_end(b"XXXXX\r\n\r\nXXXXXXX"), Some(9));
		assert_eq!(scan_for_header_end(b"XXXXXX\r\n\r\nXXXXXX"), Some(10));
		assert_eq!(scan_for_header_end(b"XXXXXXX\r\n\r\nXXXXX"), Some(11));
		assert_eq!(scan_for_header_end(b"XXXXXXXX\r\n\r\nXXXX"), Some(12));
		assert_eq!(scan_for_header_end(b"XXXXXXXXX\r\n\r\nXXX"), Some(13));
		assert_eq!(scan_for_header_end(b"XXXXXXXXXX\r\n\r\nXX"), Some(14));
		assert_eq!(scan_for_header_end(b"XXXXXXXXXXX\r\n\r\nX"), Some(15));
		assert_eq!(scan_for_header_end(b"XXXXXXXXXXXX\r\n\r\n"), Some(16));
		assert_eq!(
			scan_for_header_end(b"XXXXXXXXXXXXX\r\n\r\nXXXXXXXXXXXXXXX"),
			Some(17)
		);
		assert_eq!(
			scan_for_header_end(b"XX\nXXXXXXXXXXXXXXXXXXXXXXXXX\r\n\r\n"),
			Some(32)
		);
		assert_eq!(
			scan_for_header_end(b"XX\r\nXXXXXXXXXX\r\n\r\nXXXXXXXXXXXXXX"),
			Some(18)
		);

		assert_eq!(
			scan_for_header_end(b"Header: value\r\n\nAnother: value\r\n\r\n"),
			Some(34)
		);
	}

	#[test]
	fn test_scan_for_header_end_not_found() {
		assert_eq!(scan_for_header_end(b"XXXXXXXXXXXXXXXX"), None);
		assert_eq!(
			scan_for_header_end(b"XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX"),
			None
		);
		assert_eq!(scan_for_header_end(b"\rX\nXXX\rXXX\rXX\nX\n"), None);
		assert_eq!(scan_for_header_end(b"Header: value\r\n"), None);
		assert_eq!(
			scan_for_header_end(b"X\nXX\nXX\nXX\nXX\nXX\nXX\nXX\nXX\nXX\nXX\n"),
			None
		);
		assert_eq!(
			scan_for_header_end(b"X\nXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX"),
			None
		);
		assert_eq!(
			scan_for_header_end(
				b"\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n"
			),
			None
		);
	}
}
