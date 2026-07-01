//! Server-Sent Event (SSE) utilities.
//!
//! This module primarily provides the [`SseEvent`] struct and associated methods for creating and
//! serializing SSE events.
//!
//! # Examples
//!
//! ```
//! use moonbeam::SseEvent;
//!
//! let event = SseEvent::new()
//!     .with_event("message")
//!     .with_data("Hello, world!");
//! assert_eq!(event.to_string(), "event: message\ndata: Hello, world!\n\n");
//! ```

use std::fmt;

/// A Server-Sent Event (SSE) structure.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SseEvent {
	/// Optional event ID to set the EventSource's last event ID value.
	pub id: Option<String>,
	/// Optional event name/type (e.g. "ping", "message").
	pub event: Option<String>,
	/// The event data payload. Can be multi-line.
	pub data: Option<String>,
	/// Optional reconnection time in milliseconds.
	pub retry: Option<u64>,
}

impl SseEvent {
	/// Creates a new empty `SseEvent`. If nothing is set, this will serialize to a single `'\n'`.
	pub fn new() -> Self {
		Self {
			id: None,
			event: None,
			data: None,
			retry: None,
		}
	}

	/// Sets the event name/type.
	pub fn with_event(mut self, event: impl Into<String>) -> Self {
		self.event = Some(event.into());
		self
	}

	/// Sets the event ID.
	pub fn with_id(mut self, id: impl Into<String>) -> Self {
		self.id = Some(id.into());
		self
	}

	/// Sets the retry/reconnection time in milliseconds.
	pub fn with_retry(mut self, retry: u64) -> Self {
		self.retry = Some(retry);
		self
	}

	/// Sets the event data payload.
	///
	/// If this payload is not set, browsers may ignore the event. Set to an empty string (or a
	/// value) to ensure delivery.
	pub fn with_data(mut self, data: impl Into<String>) -> Self {
		self.data = Some(data.into());
		self
	}
}

impl fmt::Display for SseEvent {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		if let Some(ref id) = self.id {
			writeln!(f, "id: {}", id)?;
		}
		if let Some(ref event) = self.event {
			writeln!(f, "event: {}", event)?;
		}
		if let Some(ref data) = self.data {
			if data.is_empty() {
				writeln!(f, "data:")?;
			} else {
				for line in data.lines() {
					writeln!(f, "data: {}", line)?;
				}
			}
		}
		if let Some(retry) = self.retry {
			writeln!(f, "retry: {}", retry)?;
		}
		writeln!(f)
	}
}

impl From<SseEvent> for Vec<u8> {
	fn from(event: SseEvent) -> Self {
		event.to_string().into_bytes()
	}
}

impl From<&SseEvent> for Vec<u8> {
	fn from(event: &SseEvent) -> Self {
		event.to_string().into_bytes()
	}
}

#[cfg(test)]
mod test {
	use super::*;

	#[test]
	fn test_sse_event() {
		let ev = SseEvent::new()
			.with_data("line1\nline2")
			.with_event("ping")
			.with_id("123")
			.with_retry(5000);

		let serialized = ev.to_string();
		assert_eq!(
			serialized,
			"id: 123\nevent: ping\ndata: line1\ndata: line2\nretry: 5000\n\n"
		);
	}

	#[test]
	fn test_sse_event_empty_data() {
		let ev = SseEvent::new().with_data("").with_event("ping");

		let serialized = ev.to_string();
		assert_eq!(serialized, "event: ping\ndata:\n\n");
	}
}
