use async_executor::LocalExecutor;

pub(super) fn get_local_executor() -> &'static LocalExecutor<'static> {
	thread_local! {
		static EXECUTOR: LocalExecutor = const { LocalExecutor::new() };
	}

	EXECUTOR.with(|ex| {
		// SAFETY: The thread-local executor lives for the entire thread lifetime
		unsafe { std::mem::transmute(ex) }
	})
}

/// Spawns a new task on the local executor.
///
/// This function spawns a task that runs on the current thread's executor.
/// The task is detached and will run concurrently with other tasks.
///
/// If the `signals` feature is enabled, this task is tracked for graceful shutdown.
#[cfg(feature = "signals")]
pub fn new_local_task<T: 'static>(future: impl Future<Output = T> + 'static) {
	use super::task_tracker::get_local_tracker;

	let guard = get_local_tracker().track();
	get_local_executor()
		.spawn(async {
			let _guard = guard;
			future.await
		})
		.detach();
}

/// Spawns a new task on the local executor.
///
/// This function spawns a task that runs on the current thread's executor.
/// The task is detached and will run concurrently with other tasks.
#[cfg(not(feature = "signals"))]
pub fn new_local_task<T: 'static>(future: impl Future<Output = T> + 'static) {
	get_local_executor().spawn(future).detach();
}
