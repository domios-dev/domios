use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub struct WorkerPool {
    max_per_provider: usize,
    slots: Mutex<HashMap<String, Arc<Semaphore>>>,
}

impl WorkerPool {
    pub fn new(max_per_provider: usize) -> Self {
        let n = max_per_provider.max(1);
        Self {
            max_per_provider: n,
            slots: Mutex::new(HashMap::new()),
        }
    }

    pub async fn acquire(&self, provider: &str) -> OwnedSemaphorePermit {
        let sem = self.slot(provider);
        sem.acquire_owned()
            .await
            .expect("worker pool semaphore closed")
    }

    pub fn available_permits(&self, provider: &str) -> usize {
        self.slots
            .lock()
            .get(provider)
            .map(|s| s.available_permits())
            .unwrap_or(self.max_per_provider)
    }

    pub fn max_per_provider(&self) -> usize {
        self.max_per_provider
    }

    fn slot(&self, provider: &str) -> Arc<Semaphore> {
        let mut slots = self.slots.lock();
        slots
            .entry(provider.to_string())
            .or_insert_with(|| Arc::new(Semaphore::new(self.max_per_provider)))
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn caps_at_max_per_provider() {
        let pool = WorkerPool::new(2);
        let a = pool.acquire("claude").await;
        let b = pool.acquire("claude").await;
        assert_eq!(pool.available_permits("claude"), 0);
        drop(a);
        assert_eq!(pool.available_permits("claude"), 1);
        drop(b);
        assert_eq!(pool.available_permits("claude"), 2);
    }

    #[tokio::test]
    async fn providers_are_independent() {
        let pool = WorkerPool::new(1);
        let _c = pool.acquire("claude").await;
        assert_eq!(pool.available_permits("claude"), 0);
        // codex has its own slot
        let _o = pool.acquire("codex").await;
        assert_eq!(pool.available_permits("codex"), 0);
        assert_eq!(pool.available_permits("gemini"), 1);
    }

    #[tokio::test]
    async fn queued_acquires_wait_for_release() {
        let pool = Arc::new(WorkerPool::new(1));
        let a = pool.acquire("claude").await;

        let pool_clone = pool.clone();
        let waiter = tokio::spawn(async move {
            let _b = pool_clone.acquire("claude").await;
            "got it"
        });

        // Should still be blocked
        assert_eq!(pool.available_permits("claude"), 0);

        drop(a);
        let result = tokio::time::timeout(std::time::Duration::from_secs(1), waiter)
            .await
            .expect("waiter didn't complete");
        assert_eq!(result.unwrap(), "got it");
    }
}
