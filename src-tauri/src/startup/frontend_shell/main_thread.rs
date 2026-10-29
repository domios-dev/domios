use super::*;

const QUEUED: u8 = 0;
const RUNNING: u8 = 1;
const CANCELLED: u8 = 2;
const COMPLETED: u8 = 3;

struct CancelQueued(Arc<AtomicU8>);

impl Drop for CancelQueued {
    fn drop(&mut self) {
        let _ = self
            .0
            .compare_exchange(QUEUED, CANCELLED, Ordering::AcqRel, Ordering::Acquire);
    }
}

pub(super) async fn run_on_main_thread<T: Send + 'static>(
    app: codeterm_client_shell::app_runtime::AppHandle,
    action: impl FnOnce(codeterm_client_shell::app_runtime::AppHandle) -> Result<T, String>
        + Send
        + 'static,
) -> Result<T, String> {
    let (sender, mut receiver) = tokio::sync::oneshot::channel();
    let action_app = app.clone();
    let phase = Arc::new(AtomicU8::new(QUEUED));
    let _cancel_queued = CancelQueued(phase.clone());
    let scheduled_phase = phase.clone();
    app.run_on_main_thread(move || {
        if scheduled_phase
            .compare_exchange(QUEUED, RUNNING, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let result = action(action_app);
        scheduled_phase.store(COMPLETED, Ordering::Release);
        let _ = sender.send(result);
    })
    .map_err(|error| error.to_string())?;
    match tokio::time::timeout(Duration::from_secs(10), &mut receiver).await {
        Ok(Ok(result)) => result,
        Ok(Err(_)) => {
            if phase
                .compare_exchange(QUEUED, CANCELLED, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                Err("Main-thread frontend action was cancelled before execution".into())
            } else {
                Err("Main-thread frontend action outcome is uncertain after execution began".into())
            }
        }
        Err(_) => {
            if phase
                .compare_exchange(QUEUED, CANCELLED, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Err("Main-thread frontend action deadline elapsed before execution".into());
            }
            match receiver.try_recv() {
                Ok(result) => result,
                Err(_) => Err("Main-thread frontend action acknowledgement is uncertain".into()),
            }
        }
    }
}

#[cfg(test)]
mod tests;
