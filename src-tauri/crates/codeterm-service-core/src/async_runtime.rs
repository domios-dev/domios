use std::future::Future;
use std::sync::OnceLock;

use tokio::runtime::Handle;
pub use tokio::task::JoinHandle;

static RUNTIME: OnceLock<Handle> = OnceLock::new();

pub fn install(handle: Handle) {
    let id = handle.id();
    assert_eq!(
        RUNTIME.get_or_init(|| handle).id(),
        id,
        "service runtime is already installed with a different host runtime"
    );
}

pub fn handle() -> &'static Handle {
    if let Some(handle) = RUNTIME.get() {
        return handle;
    }
    #[cfg(any(test, feature = "test-support"))]
    {
        static TEST_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
        TEST_RUNTIME
            .get_or_init(|| {
                tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .expect("test runtime initialization failed")
            })
            .handle()
    }
    #[cfg(not(any(test, feature = "test-support")))]
    panic!("service runtime is not installed; install the host runtime before starting services")
}

pub fn spawn<F>(task: F) -> JoinHandle<F::Output>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    handle().spawn(task)
}

pub fn spawn_blocking<F, R>(task: F) -> JoinHandle<R>
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    handle().spawn_blocking(task)
}

pub fn block_on<F: Future>(task: F) -> F::Output {
    handle().block_on(task)
}

#[cfg(test)]
mod tests;
