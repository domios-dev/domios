use super::DeliveryError;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

pub(super) type PayloadEnqueued = Arc<dyn Fn() + Send + Sync>;

pub(super) enum UserWrite {
    Enqueued,
    Finished,
}

enum WriteEvent {
    Enqueued,
    Finished(Result<(), DeliveryError>),
}

impl crate::service::ServiceContext {
    pub(super) fn run_write_handoff(
        &self,
        tab_id: &str,
        submit: impl FnOnce(PayloadEnqueued) -> Result<(), DeliveryError> + Send + 'static,
        complete: impl FnOnce(Result<(), DeliveryError>) + Send + 'static,
    ) -> Result<UserWrite, DeliveryError> {
        let (events, receiver) = mpsc::channel();
        let enqueued = Arc::new(AtomicBool::new(false));
        let payload_enqueued = {
            let enqueued = enqueued.clone();
            let events = events.clone();
            Arc::new(move || {
                if !enqueued.swap(true, Ordering::AcqRel) {
                    let _ = events.send(WriteEvent::Enqueued);
                }
            }) as PayloadEnqueued
        };
        let tab = tab_id.to_owned();
        #[cfg(any(test, feature = "test-support"))]
        let completion = self.ready_user_send_tasks.start(tab_id);
        crate::async_runtime::spawn_blocking(move || {
            #[cfg(any(test, feature = "test-support"))]
            let _completion = completion;
            let outcome = submit(payload_enqueued);
            if enqueued.load(Ordering::Acquire) {
                let outcome = outcome.map_err(|error| {
                    if error.bytes_written() {
                        error
                    } else {
                        DeliveryError::SubmitUnconfirmed {
                            tab,
                            detail: error.ledger_message(),
                        }
                    }
                });
                complete(outcome);
            } else {
                let _ = events.send(WriteEvent::Finished(outcome));
            }
        });
        match receiver.recv() {
            Ok(WriteEvent::Enqueued) => Ok(UserWrite::Enqueued),
            Ok(WriteEvent::Finished(result)) => result.map(|()| UserWrite::Finished),
            Err(_) => Err(DeliveryError::SubmitUnconfirmed {
                tab: tab_id.into(),
                detail: "write handoff ended without a classified result".into(),
            }),
        }
    }
}
