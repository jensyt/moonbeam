#[cfg(feature = "tracing")]
#[allow(unused_imports)]
pub use tracing::{
	Instrument, Span, debug, debug_span, error, field, info, info_span, trace, warn,
};

#[cfg(not(feature = "tracing"))]
mod tracing_impl {
	#[derive(Clone, Copy)]
	pub struct Span;
	impl Span {
		#[allow(unused)]
		pub fn entered(self) -> Self {
			self
		}

		#[allow(unused)]
		pub fn current() -> Self {
			self::Span
		}

		#[allow(unused)]
		pub fn record<Q: ?Sized, V>(&self, _field: &Q, _value: V) -> &Self {
			self
		}
	}

	pub trait Instrument: Sized {
		fn instrument(self, _span: Span) -> Self {
			self
		}
	}

	impl<T: Sized> Instrument for T {}

	pub mod field {
		#[allow(dead_code)]
		pub struct Empty;
	}

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
	macro_rules! debug_span {
		($($arg:tt)*) => {
			$crate::tracing::Span
		};
	}
}

#[cfg(not(feature = "tracing"))]
#[allow(unused_imports)]
pub use {
	crate::debug, crate::debug_span, crate::error, crate::info, crate::info_span, crate::trace,
	crate::warn, tracing_impl::Instrument, tracing_impl::Span, tracing_impl::field,
};
