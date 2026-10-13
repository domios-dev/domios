use std::{future::Future, time::Duration};

pub(crate) fn deadline<F: Future>(
    duration: Duration,
    future: F,
) -> Result<F::Output, tokio::time::error::Elapsed> {
    tauri::async_runtime::block_on(async { tokio::time::timeout(duration, future).await })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_permit_deadline_enters_runtime_before_constructing_timer() {
        std::thread::spawn(|| {
            assert!(tokio::runtime::Handle::try_current().is_err());
            let gate = tokio::sync::Semaphore::new(1);
            let permit = deadline(Duration::from_secs(1), gate.acquire())
                .unwrap()
                .unwrap();
            assert_eq!(gate.available_permits(), 0);
            drop(permit);
            assert_eq!(gate.available_permits(), 1);
        })
        .join()
        .unwrap();
    }
}
