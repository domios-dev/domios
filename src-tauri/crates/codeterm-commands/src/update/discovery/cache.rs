use std::future::Future;

pub struct SingleFlight<T> {
    gate: tokio::sync::Mutex<()>,
    completed: parking_lot::Mutex<(u64, Option<Result<T, String>>)>,
}

impl<T: Clone> SingleFlight<T> {
    pub const fn new() -> Self {
        Self {
            gate: tokio::sync::Mutex::const_new(()),
            completed: parking_lot::Mutex::new((0, None)),
        }
    }

    pub fn generation(&self) -> u64 {
        self.completed.lock().0
    }

    pub async fn run<F, Fut>(&self, generation: u64, refresh: F) -> Result<T, String>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, String>>,
    {
        let _guard = self.gate.lock().await;
        {
            let completed = self.completed.lock();
            if completed.0 != generation {
                if let Some(result) = &completed.1 {
                    return result.clone();
                }
            }
        }
        let result = refresh().await;
        let mut completed = self.completed.lock();
        completed.0 = completed.0.wrapping_add(1);
        completed.1 = Some(result.clone());
        result
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn fresh(checked_at_ms: Option<u64>, now_ms: u64) -> bool {
    checked_at_ms
        .and_then(|checked| now_ms.checked_sub(checked))
        .is_some_and(|age| age < 30_000)
}

#[cfg(test)]
mod tests;
