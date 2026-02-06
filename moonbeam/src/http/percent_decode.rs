use std::borrow::Cow;

// Lookup table for fast hex decoding.
// 0xFF indicates invalid character.
const HEX_TABLE: [u8; 256] = {
	let mut table = [0xFF; 256];
	let mut i = 0u8;
	while i < 10 {
		table[(b'0' + i) as usize] = i;
		i += 1;
	}
	let mut i = 0u8;
	while i < 6 {
		table[(b'a' + i) as usize] = 10 + i;
		table[(b'A' + i) as usize] = 10 + i;
		i += 1;
	}
	table
};

/// Decodes percent-encoded characters.
///
/// This function avoids allocations if no decoding is needed (no '%' characters).
pub fn decode(s: &str) -> Cow<'_, str> {
	match scan(s.as_bytes(), false) {
		Some(i) => decode_internal(s, i, false),
		None => Cow::Borrowed(s),
	}
}

/// Decodes percent-encoded characters and converts '+' to ' '.
///
/// This function avoids allocations if no decoding is needed (no '%' or '+' characters).
pub fn decode_query(s: &str) -> Cow<'_, str> {
	match scan(s.as_bytes(), true) {
		Some(i) => decode_internal(s, i, true),
		None => Cow::Borrowed(s),
	}
}

#[inline]
fn scan(s: &[u8], plus_to_space: bool) -> Option<usize> {
	let mut i = 0;
	while i < s.len() {
		let b = s[i];
		if b == b'%' || (plus_to_space && b == b'+') {
			return Some(i);
		}
		i += 1;
	}
	None
}

fn decode_internal(s: &str, start_index: usize, plus_to_space: bool) -> Cow<'_, str> {
	let bytes = s.as_bytes();
	let mut decoded = Vec::with_capacity(bytes.len());

	// Copy the part that didn't need decoding.
	decoded.extend_from_slice(&bytes[..start_index]);

	let mut i = start_index;
	let mut changed = false;
	let mut all_ascii_inserted = true;

	while i < bytes.len() {
		// Optimistically find the next special char to bulk copy
		let next_special = match scan(&bytes[i..], plus_to_space) {
			Some(offset) => i + offset,
			None => bytes.len(),
		};

		if next_special > i {
			decoded.extend_from_slice(&bytes[i..next_special]);
			i = next_special;
		}

		if i >= bytes.len() {
			break;
		}

		match bytes[i] {
			b'+' if plus_to_space => {
				decoded.push(b' ');
				i += 1;
				changed = true;
			}
			b'%' => {
				if i + 2 < bytes.len() {
					let h = HEX_TABLE[bytes[i + 1] as usize];
					let l = HEX_TABLE[bytes[i + 2] as usize];
					if h != 0xFF && l != 0xFF {
						let b = (h << 4) | l;
						if b >= 128 {
							all_ascii_inserted = false;
						}
						decoded.push(b);
						i += 3;
						changed = true;
						continue;
					}
				}
				decoded.push(b'%');
				i += 1;
			}
			b => {
				decoded.push(b);
				i += 1;
			}
		}
	}

	if !changed {
		return Cow::Borrowed(s);
	}

	if all_ascii_inserted {
		// SAFETY: The original string was valid UTF-8, and we only inserted ASCII bytes
		// at positions where ASCII characters are allowed (which is everywhere in UTF-8
		// except inside multi-byte sequences, but % and + cannot appear there).
		unsafe { Cow::Owned(String::from_utf8_unchecked(decoded)) }
	} else {
		match String::from_utf8(decoded) {
			Ok(s) => Cow::Owned(s),
			Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned().into(),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_decode() {
		assert_eq!(decode("foo"), Cow::Borrowed("foo"));
		assert_eq!(decode("foo%20bar"), Cow::Owned::<str>("foo bar".into()));
		assert_eq!(decode("foo+bar"), Cow::Borrowed("foo+bar"));
	}

	#[test]
	fn test_decode_query() {
		assert_eq!(decode_query("foo"), Cow::Borrowed("foo"));

		assert_eq!(decode_query("foo+bar"), Cow::Owned::<str>("foo bar".into()));
		assert_eq!(
			decode_query("foo%20bar"),
			Cow::Owned::<str>("foo bar".into())
		);
		assert_eq!(
			decode_query("foo%2bbar"),
			Cow::Owned::<str>("foo+bar".into())
		);
		assert_eq!(
			decode_query("foo+bar%20baz"),
			Cow::Owned::<str>("foo bar baz".into())
		);
		assert_eq!(decode_query("%G%H"), Cow::Borrowed("%G%H")); // invalid hex, no change
		assert_eq!(decode_query("percent%"), Cow::Borrowed("percent%")); // trailing %, no change
	}

	#[test]
	fn test_decode_query_utf8() {
		// %F0%9F%90%80 is 🐀
		assert_eq!(decode_query("%F0%9F%90%80"), Cow::Owned::<str>("🐀".into()));
	}

	#[test]
	fn test_decode_query_invalid_utf8() {
		// 0xFF is invalid UTF-8
		let decoded = decode_query("%FF");
		assert_eq!(decoded, Cow::Owned::<str>("\u{FFFD}".into()));
	}
}
