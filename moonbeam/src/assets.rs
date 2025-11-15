use crate::{Response, http::Body};
use std::{fs::File, path::Path};

pub fn get_asset(path: &str, root: impl AsRef<Path>) -> Response {
	let root = match root.as_ref().canonicalize() {
		Ok(p) => p,
		Err(_) => return Response::internal_server_error(),
	};

	let path = path.trim_start_matches('/');
	let path = match root.join(path).canonicalize() {
		Ok(p) => p,
		Err(_) => return Response::not_found(),
	};

	if !path.starts_with(root) || !path.is_file() {
		return Response::not_found();
	}

	let ext = get_mime_type(&path);

	let file = match File::open(path) {
		Ok(f) => f,
		Err(_) => return Response::internal_server_error(),
	};

	#[cfg(feature = "asyncfs")]
	{
		Response::new_with_body(Body::from_file_async(file), ext)
	}
	#[cfg(not(feature = "asyncfs"))]
	{
		Response::new_with_body(file, ext)
	}
}

/// Returns the MIME type for a given file path.
///
/// # Arguments
/// * `path` - A reference to a Path
///
/// # Returns
/// * `Option<&'static str>` - The MIME type if recognized, None otherwise
pub fn get_mime_type<P>(path: &P) -> Option<&'static str>
where
	P: AsRef<Path> + ?Sized,
{
	let ext = path.as_ref().extension()?.to_str()?;
	let ext_lower = ext.to_lowercase();

	match ext_lower.as_str() {
		// Text files
		"txt" => Some("text/plain"),
		"html" | "htm" => Some("text/html"),
		"css" => Some("text/css"),
		"js" | "mjs" => Some("text/javascript"),
		"csv" => Some("text/csv"),
		"xml" => Some("text/xml"),
		"md" | "markdown" => Some("text/markdown"),
		"rtf" => Some("application/rtf"),
		"tex" => Some("application/x-tex"),

		// Image files
		"jpg" | "jpeg" => Some("image/jpeg"),
		"png" => Some("image/png"),
		"gif" => Some("image/gif"),
		"svg" => Some("image/svg+xml"),
		"webp" => Some("image/webp"),
		"bmp" => Some("image/bmp"),
		"tif" | "tiff" => Some("image/tiff"),
		"ico" => Some("image/x-icon"),
		"heic" | "heif" => Some("image/heif"),
		"avif" => Some("image/avif"),

		// Audio files
		"mp3" => Some("audio/mpeg"),
		"wav" => Some("audio/wav"),
		"ogg" | "oga" => Some("audio/ogg"),
		"weba" => Some("audio/webm"),
		"aac" => Some("audio/aac"),
		"flac" => Some("audio/flac"),
		"m4a" => Some("audio/mp4"),
		"opus" => Some("audio/opus"),

		// Video files
		"mp4" | "m4v" => Some("video/mp4"),
		"mpeg" | "mpg" => Some("video/mpeg"),
		"webm" => Some("video/webm"),
		"ogv" => Some("video/ogg"),
		"avi" => Some("video/x-msvideo"),
		"mov" | "qt" => Some("video/quicktime"),
		"mkv" => Some("video/x-matroska"),
		"flv" => Some("video/x-flv"),
		"wmv" => Some("video/x-ms-wmv"),

		// Application files
		"pdf" => Some("application/pdf"),
		"zip" => Some("application/zip"),
		"rar" => Some("application/x-rar-compressed"),
		"json" => Some("application/json"),
		"jsonld" => Some("application/ld+json"),
		"doc" => Some("application/msword"),
		"docx" => Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
		"xls" => Some("application/vnd.ms-excel"),
		"xlsx" => Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
		"ppt" => Some("application/vnd.ms-powerpoint"),
		"pptx" => Some("application/vnd.openxmlformats-officedocument.presentationml.presentation"),
		"7z" => Some("application/x-7z-compressed"),
		"tar" => Some("application/x-tar"),
		"gz" | "gzip" => Some("application/gzip"),
		"bz" | "bz2" => Some("application/x-bzip2"),
		"apk" => Some("application/vnd.android.package-archive"),
		"jar" => Some("application/java-archive"),
		"war" => Some("application/java-archive"),
		"exe" => Some("application/x-msdownload"),
		"dmg" => Some("application/x-apple-diskimage"),
		"deb" => Some("application/x-debian-package"),
		"rpm" => Some("application/x-rpm"),
		"bin" | "dll" | "so" => Some("application/octet-stream"),
		"wasm" => Some("application/wasm"),
		"sh" => Some("application/x-sh"),
		"sql" => Some("application/sql"),
		"yaml" | "yml" => Some("application/x-yaml"),
		"toml" => Some("application/toml"),

		// Font files
		"woff" => Some("font/woff"),
		"woff2" => Some("font/woff2"),
		"ttf" => Some("font/ttf"),
		"otf" => Some("font/otf"),

		// Unknown extension
		_ => None,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_common_extensions() {
		assert_eq!(get_mime_type(Path::new("index.html")), Some("text/html"));
		assert_eq!(get_mime_type(Path::new("photo.jpg")), Some("image/jpeg"));
		assert_eq!(
			get_mime_type(Path::new("document.pdf")),
			Some("application/pdf")
		);
		assert_eq!(get_mime_type(Path::new("song.mp3")), Some("audio/mpeg"));
	}

	#[test]
	fn test_case_insensitive() {
		assert_eq!(get_mime_type(Path::new("file.HTML")), Some("text/html"));
		assert_eq!(get_mime_type(Path::new("image.JpG")), Some("image/jpeg"));
	}

	#[test]
	fn test_unknown_extension() {
		assert_eq!(get_mime_type(Path::new("file.unknown")), None);
	}

	#[test]
	fn test_no_extension() {
		assert_eq!(get_mime_type(Path::new("README")), None);
	}

	#[test]
	fn test_with_directory() {
		assert_eq!(
			get_mime_type(Path::new("/path/to/file.json")),
			Some("application/json")
		);
	}
}
