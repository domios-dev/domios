pub async fn run<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| format!("blocking service task failed: {error}"))?
}

#[cfg(test)]
pub(crate) fn install_test_runtime() {
    crate::async_runtime::install(tauri::async_runtime::handle().inner().clone());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn nested_runtime_reproduces_on_async_worker() {
        install_test_runtime();
        let panic =
            std::panic::catch_unwind(|| crate::async_runtime::block_on(async { 42 })).unwrap_err();
        let message = panic
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied())
            .unwrap();
        assert!(message.contains("Cannot start a runtime from within a runtime"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn nested_runtime_runs_off_async_worker() {
        install_test_runtime();
        let worker = std::thread::current().id();
        let result = run(move || {
            assert_ne!(std::thread::current().id(), worker);
            Ok(crate::async_runtime::block_on(async { 42 }))
        })
        .await
        .unwrap();
        assert_eq!(result, 42);
    }
}
