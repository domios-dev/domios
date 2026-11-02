pub async fn run<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| format!("blocking service task failed: {error}"))?
}

#[cfg(test)]
pub(crate) fn install_test_runtime() {
    let _ = crate::async_runtime::handle();
}

#[cfg(test)]
mod tests {
    use super::*;

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
