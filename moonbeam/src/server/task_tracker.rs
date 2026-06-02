use crate::tracing;
use async_io::Timer;
use std::{cell::Cell, marker::PhantomData, time::Duration};

pub(super) struct TaskTracker {
	count: Cell<usize>,
	// Make this type !Send since it is meant to be thread-local only
	_phantom: PhantomData<*const ()>,
}

impl TaskTracker {
	pub const fn new() -> Self {
		TaskTracker {
			count: Cell::new(0),
			_phantom: PhantomData,
		}
	}

	#[must_use]
	pub fn track(&self) -> TaskGuard<'_> {
		TaskGuard::new(self)
	}

	pub async fn wait_until_empty(&self, mut timeout: Duration) {
		let ms = Duration::from_millis(100);
		let mut count = 0;
		while self.count.get() > 0 && timeout > Duration::ZERO {
			Timer::after(ms).await;
			timeout = timeout.saturating_sub(ms);
			count += 1;
			if count % 10 == 0 {
				tracing::trace!(
					task_count = self.count.get(),
					"Task tracker shutdown waiting {}s",
					count / 10
				);
			}
		}
	}
}

pub(super) struct TaskGuard<'t> {
	tracker: &'t TaskTracker,
}

impl<'t> TaskGuard<'t> {
	pub fn new(tracker: &'t TaskTracker) -> Self {
		tracker.count.set(tracker.count.get() + 1);
		tracing::trace!(count = tracker.count.get(), "Task guard created");
		TaskGuard { tracker }
	}
}

impl<'t> Drop for TaskGuard<'t> {
	fn drop(&mut self) {
		self.tracker.count.set(self.tracker.count.get() - 1);
		tracing::trace!(count = self.tracker.count.get(), "Task guard dropped");
	}
}
