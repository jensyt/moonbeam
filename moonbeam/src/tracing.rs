#[cfg(feature = "tracing")]
pub use tracing::{info, debug, error, warn, trace, instrument};

#[cfg(not(feature = "tracing"))]
#[macro_export]
#[doc(hidden)]
macro_rules! trace { ($($arg:tt)*) => {} }
#[cfg(not(feature = "tracing"))]
#[macro_export]
#[doc(hidden)]
macro_rules! info { ($($arg:tt)*) => {} }
#[cfg(not(feature = "tracing"))]
#[macro_export]
#[doc(hidden)]
macro_rules! debug { ($($arg:tt)*) => {} }
#[cfg(not(feature = "tracing"))]
#[macro_export]
#[doc(hidden)]
macro_rules! warn { ($($arg:tt)*) => {} }
#[cfg(not(feature = "tracing"))]
#[macro_export]
#[doc(hidden)]
macro_rules! error { ($($arg:tt)*) => {} }

#[cfg(not(feature = "tracing"))]
#[allow(unused_imports)]
pub(crate) use {trace, info, debug, crate::warn, error};
