use async_executor::StaticLocalExecutor;
use super::task_tracker::get_local_tracker;

pub(super) fn get_local_executor() -> &'static StaticLocalExecutor {
	thread_local! {
		static EXECUTOR: StaticLocalExecutor = const { StaticLocalExecutor::new() };
	}

	EXECUTOR.with(|ex| {
		// SAFETY: The thread-local executor lives for the entire thread lifetime
		unsafe { std::mem::transmute(ex) }
	})
}

pub fn new_local_task<T: 'static>(future: impl Future<Output = T> + 'static) {
	let guard = get_local_tracker().track();
	get_local_executor()
		.spawn(async {
			let _guard = guard;
			future.await
		})
		.detach();
}
