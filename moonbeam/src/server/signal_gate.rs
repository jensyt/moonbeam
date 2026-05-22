use super::task::Spawner;
use std::future::Future;
use std::io::Error;

#[cfg(feature = "signals")]
mod signals {
	use super::*;
	use crate::tracing;
	use async_signal::{Signal, Signals};
	use futures_lite::{FutureExt, StreamExt};
	use std::io::ErrorKind;
	use std::time::Duration;

	pub struct SignalGate {
		signals: async_signal::Signals,
	}

	impl SignalGate {
		pub fn new() -> Self {
			let signals =
				Signals::new([Signal::Int, Signal::Term]).expect("Failed to create signal handler");
			Self { signals }
		}

		pub async fn or_signal<F, O>(&mut self, fut: F) -> Result<O, Error>
		where
			F: Future<Output = Result<O, Error>>,
		{
			let signal_err = async {
				let signal = self.signals.next().await;
				Err(Error::new(
					ErrorKind::Interrupted,
					format!("Signal: {signal:?}"),
				))
			};
			fut.or(signal_err).await
		}

		pub async fn wait_for_shutdown(mut self, spawner: Spawner<'_>) {
			let wait_for_tasks = spawner.wait_until_empty(Duration::from_secs(60));
			let force_shutdown = async {
				if let Some(_signal) = self.signals.next().await {
					tracing::warn!(?_signal, "Received signal, forcing shutdown");
				}
			};
			wait_for_tasks.or(force_shutdown).await;
		}
	}
}
#[cfg(feature = "signals")]
pub(in crate::server) use signals::SignalGate;

#[cfg(not(feature = "signals"))]
mod nosignals {
	use super::*;

	pub(in crate::server) struct SignalGate;

	impl SignalGate {
		pub(in crate::server) fn new() -> Self {
			Self
		}

		pub(in crate::server) fn or_signal<F, O>(&mut self, fut: F) -> F
		where
			F: Future<Output = Result<O, Error>>,
		{
			fut
		}

		pub(in crate::server) async fn wait_for_shutdown(self, _spawner: Spawner<'_>) {}
	}
}
#[cfg(not(feature = "signals"))]
pub(in crate::server) use nosignals::SignalGate;
