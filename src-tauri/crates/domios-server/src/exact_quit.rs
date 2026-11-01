use crate::native_host::NativeHostEndpoint;
use codeterm_api::app_quit_routes::{AppQuitPort, QuitReplyFuture};
use codeterm_types::{
    app_quit::{
        QuitApiReply, QuitCompletion, QuitRefusal, QuitSnapshot, QuitUncertainty,
        QUIT_FAILURE_DEADLINE,
    },
    native_host::{NativeHostCompletion, NativeHostOperation, NativeHostResult, NativeHostScope},
    native_update::{NativeUpdateRequest, NativeUpdateResult},
    server_bootstrap::ServerReady,
};
use parking_lot::Mutex;
use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::sync::{mpsc, oneshot};

type Endpoint = NativeHostEndpoint<NativeUpdateRequest, NativeUpdateResult>;
struct Pending {
    expected: QuitSnapshot,
    response: oneshot::Sender<QuitApiReply>,
}
#[derive(Default)]
struct State {
    ready: Option<ServerReady>,
    endpoint: Option<Endpoint>,
    pending: Option<Pending>,
    admitted: bool,
}
pub(crate) struct ExactQuitCoordinator {
    state: Mutex<State>,
    headless: mpsc::Sender<QuitSnapshot>,
    completion_sent: AtomicBool,
}
impl ExactQuitCoordinator {
    pub(crate) fn new() -> (Arc<Self>, mpsc::Receiver<QuitSnapshot>) {
        let (headless, receiver) = mpsc::channel(1);
        (
            Arc::new(Self {
                state: Mutex::new(State::default()),
                headless,
                completion_sent: AtomicBool::new(false),
            }),
            receiver,
        )
    }
    pub(crate) fn install(&self, ready: ServerReady, endpoint: Option<Endpoint>) {
        let mut state = self.state.lock();
        state.ready = Some(ready);
        state.endpoint = endpoint;
    }
    pub(crate) fn complete(&self, result: &io::Result<()>) {
        let pending = self.state.lock().pending.take();
        if let Some(pending) = pending {
            let reply = match result {
                Ok(()) => QuitApiReply::Completed {
                    expected: Box::new(pending.expected),
                    shutdown_completion: QuitCompletion::default(),
                },
                Err(error) => {
                    log::debug!("[exact-quit] shutdown completion failed: {error}");
                    QuitApiReply::Uncertain {
                        reason: QuitUncertainty::InvalidCompletion,
                    }
                }
            };
            if pending.response.send(reply).is_ok() && result.is_ok() {
                self.completion_sent.store(true, Ordering::Release);
            }
        }
    }
    pub(crate) fn completion_sent(&self) -> bool {
        self.completion_sent.load(Ordering::Acquire)
    }
    fn refuse_before_dispatch(&self, reason: QuitRefusal) -> QuitApiReply {
        let mut state = self.state.lock();
        state.pending.take();
        state.admitted = false;
        QuitApiReply::Refused { reason }
    }
    pub(crate) fn admit_headless(&self, expected: &QuitSnapshot) -> bool {
        let valid = expected.client.is_none()
            && self
                .snapshot()
                .is_ok_and(|current| current.matches(expected));
        if !valid {
            let mut state = self.state.lock();
            if let Some(pending) = state.pending.take() {
                let _ = pending.response.send(QuitApiReply::Refused {
                    reason: QuitRefusal::IdentityMismatch,
                });
            }
            state.admitted = false;
        }
        valid
    }
}
impl AppQuitPort for ExactQuitCoordinator {
    fn snapshot(&self) -> Result<QuitSnapshot, QuitRefusal> {
        let (ready, endpoint) = {
            let state = self.state.lock();
            (
                state.ready.clone().ok_or(QuitRefusal::NoInstance)?,
                state.endpoint.clone(),
            )
        };
        let client = match endpoint {
            Some(endpoint) => endpoint.quit_client()?,
            None => None,
        };
        let snapshot = QuitSnapshot { ready, client };
        if snapshot.valid() {
            Ok(snapshot)
        } else {
            Err(QuitRefusal::IdentityMismatch)
        }
    }
    fn request(&self, expected: QuitSnapshot) -> QuitReplyFuture<'_> {
        Box::pin(async move {
            let actual = match self.snapshot() {
                Ok(snapshot) => snapshot,
                Err(reason) => return QuitApiReply::Refused { reason },
            };
            if actual.ready.build != expected.ready.build {
                return QuitApiReply::Refused {
                    reason: QuitRefusal::MismatchedBuild,
                };
            }
            if !actual.matches(&expected) {
                return QuitApiReply::Refused {
                    reason: QuitRefusal::IdentityMismatch,
                };
            }
            let (response, receiver) = oneshot::channel();
            let endpoint = {
                let mut state = self.state.lock();
                if state.admitted {
                    return QuitApiReply::Refused {
                        reason: QuitRefusal::Busy,
                    };
                }
                state.admitted = true;
                state.pending = Some(Pending {
                    expected: expected.clone(),
                    response,
                });
                state.endpoint.clone()
            };
            let operation = async {
                if expected.client.is_some() {
                    let Some(endpoint) = endpoint else {
                        return self.refuse_before_dispatch(QuitRefusal::Unavailable);
                    };
                    let dispatch = endpoint.request(
                        NativeHostScope::Client,
                        NativeHostOperation::ExactQuit {
                            expected: Box::new(expected.clone()),
                        },
                    );
                    tokio::pin!(receiver);
                    tokio::select! {
                        biased;
                        reply = &mut receiver => return reply.unwrap_or(QuitApiReply::Uncertain { reason: QuitUncertainty::Disconnected }),
                        outcome = dispatch => match outcome {
                            NativeHostCompletion::Completed(NativeHostResult::ExactQuitAdmitted) => {},
                            NativeHostCompletion::Completed(NativeHostResult::ExactQuitRefused(reason)) => return self.refuse_before_dispatch(reason),
                            NativeHostCompletion::Rejected(_) => return self.refuse_before_dispatch(QuitRefusal::Unavailable),
                            _ => return QuitApiReply::Uncertain { reason: QuitUncertainty::Disconnected },
                        }
                    }
                    receiver.await.unwrap_or(QuitApiReply::Uncertain {
                        reason: QuitUncertainty::Disconnected,
                    })
                } else {
                    if self.headless.try_send(expected).is_err() {
                        return self.refuse_before_dispatch(QuitRefusal::Busy);
                    }
                    receiver.await.unwrap_or(QuitApiReply::Uncertain {
                        reason: QuitUncertainty::Disconnected,
                    })
                }
            };
            tokio::time::timeout(QUIT_FAILURE_DEADLINE, operation)
                .await
                .unwrap_or(QuitApiReply::Uncertain {
                    reason: QuitUncertainty::Deadline,
                })
        })
    }
}
#[cfg(test)]
mod tests;
