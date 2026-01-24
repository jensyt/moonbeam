#[cfg(feature = "tracing")]
#[allow(unused_imports)]
pub use tracing::{Instrument, debug, error, info, info_span, trace, trace_span, warn};

#[cfg(not(feature = "tracing"))]
mod tracing_impl {
	#[derive(Clone, Copy)]
	pub struct Span;
	impl Span {
		#[allow(unused)]
		pub fn entered(self) -> Self {
			self
		}
	}

	pub trait Instrument: Sized {
		fn instrument(self, _span: Span) -> Self {
			self
		}
	}

	impl<T: Sized> Instrument for T {}

	#[macro_export]
	#[doc(hidden)]
	macro_rules! trace {
		($($arg:tt)*) => {};
	}
	#[macro_export]
	#[doc(hidden)]
	macro_rules! info {
		($($arg:tt)*) => {};
	}
	#[macro_export]
	#[doc(hidden)]
	macro_rules! debug {
		($($arg:tt)*) => {};
	}
	#[macro_export]
	#[doc(hidden)]
	macro_rules! warn {
		($($arg:tt)*) => {};
	}
	#[macro_export]
	#[doc(hidden)]
	macro_rules! error {
		($($arg:tt)*) => {};
	}
	#[macro_export]
	#[doc(hidden)]
	macro_rules! info_span {
		($($arg:tt)*) => {
			$crate::tracing::Span
		};
	}
	#[macro_export]
	#[doc(hidden)]
	macro_rules! trace_span {
		($($arg:tt)*) => {
			$crate::tracing::Span
		};
	}
}

#[cfg(not(feature = "tracing"))]
#[allow(unused_imports)]
pub(crate) use {
	crate::debug, crate::error, crate::info, crate::info_span, crate::trace, crate::trace_span,
	crate::warn, tracing_impl::Instrument, tracing_impl::Span,
};
