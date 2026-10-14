use super::model::RuntimeFailure;
use std::{
    future::Future,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

pub struct Initialization {
    ready: AtomicBool,
    permit: tokio::sync::Semaphore,
}
impl Default for Initialization {
    fn default() -> Self {
        Self {
            ready: AtomicBool::new(false),
            permit: tokio::sync::Semaphore::new(1),
        }
    }
}
impl Initialization {
    pub fn reset(&self) {
        self.ready.store(false, Ordering::Release);
    }
    pub async fn ensure<F, Fut>(&self, initialize: F) -> Result<(), RuntimeFailure>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<(), RuntimeFailure>>,
    {
        let _permit = tokio::time::timeout(Duration::from_secs(30), self.permit.acquire())
            .await
            .map_err(|_| RuntimeFailure::Deadline)?
            .map_err(|_| RuntimeFailure::Superseded)?;
        if self.ready.load(Ordering::Acquire) {
            return Ok(());
        }
        tokio::time::timeout(Duration::from_secs(30), initialize())
            .await
            .map_err(|_| RuntimeFailure::Deadline)??;
        self.ready.store(true, Ordering::Release);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    #[tokio::test]
    async fn initialize_once_serializes_callers_and_failed_handshake_stays_retryable() {
        let initialization = Initialization::default();
        let calls = AtomicUsize::new(0);
        assert!(initialization
            .ensure(|| async {
                calls.fetch_add(1, Ordering::Relaxed);
                Err(RuntimeFailure::Refused("fixture".into()))
            })
            .await
            .is_err());
        let initialize = || async {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok(())
        };
        let (one, two) = tokio::join!(
            initialization.ensure(initialize),
            initialization.ensure(initialize)
        );
        assert!(one.is_ok() && two.is_ok());
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        initialization.reset();
        initialization.ensure(initialize).await.unwrap();
        assert_eq!(calls.load(Ordering::Relaxed), 3);
    }
    #[tokio::test]
    async fn cancelled_initialize_does_not_latch_success() {
        use futures_util::FutureExt;
        let initialization = Initialization::default();
        {
            let unfinished = initialization.ensure(|| std::future::pending());
            tokio::pin!(unfinished);
            assert!(unfinished.as_mut().now_or_never().is_none());
        }
        initialization
            .ensure(|| std::future::ready(Ok(())))
            .await
            .unwrap();
        assert!(initialization.ready.load(Ordering::Acquire));
    }
}
