use super::*;
use std::future::Future;
use tokio::sync::{mpsc, watch};

pub(super) struct RecoveryOwner {
    current: watch::Sender<Option<Arc<()>>>,
    completion: parking_lot::Mutex<Option<watch::Receiver<bool>>>,
}

impl Default for RecoveryOwner {
    fn default() -> Self {
        Self {
            current: watch::channel(None).0,
            completion: parking_lot::Mutex::new(None),
        }
    }
}

struct Lease {
    identity: Arc<()>,
    changes: watch::Receiver<Option<Arc<()>>>,
    finished: watch::Sender<bool>,
}

impl Drop for Lease {
    fn drop(&mut self) {
        self.finished.send_replace(true);
    }
}

impl RecoveryOwner {
    fn owns(&self, identity: &Arc<()>) -> bool {
        self.current.borrow().as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, identity))
    }

    fn begin(&self) -> Lease {
        let identity = Arc::new(());
        let mut completion = self.completion.lock();
        let (finished, receiver) = watch::channel(false);
        let _ = self.current.send_replace(Some(identity.clone()));
        *completion = Some(receiver);
        Lease {
            identity,
            changes: self.current.subscribe(),
            finished,
        }
    }

    fn cancel(&self) {
        let _ = self.cancel_pending();
    }

    fn cancel_pending(&self) -> Option<watch::Receiver<bool>> {
        let completion = self.completion.lock();
        let _ = self.current.send_replace(None);
        completion.clone()
    }
}

impl Lease {
    fn current(&self) -> bool {
        self.changes
            .borrow()
            .as_ref()
            .is_some_and(|identity| Arc::ptr_eq(identity, &self.identity))
    }

    async fn cancelled(&self, mut exit: watch::Receiver<bool>) {
        let mut changes = self.changes.clone();
        loop {
            if !self.current() || *exit.borrow_and_update() {
                return;
            }
            tokio::select! {
                result = changes.changed() => if result.is_err() { return; },
                result = exit.changed() => if result.is_err() { return; },
            }
        }
    }
}

#[derive(Debug)]
enum Availability {
    ServerReady,
    ExactProcessExited,
}

enum AttemptFailure {
    Transient(String),
    Manual(String),
}

enum Outcome<T> {
    Ready(T),
    Failed(String),
    Cancelled,
}

async fn recover<T, F, A>(
    events: &mut mpsc::Receiver<Availability>,
    cancelled: impl Future<Output = ()>,
    mut attempt: A,
) -> Outcome<T>
where
    F: Future<Output = Result<T, AttemptFailure>>,
    A: FnMut() -> F,
{
    tokio::pin!(cancelled);
    loop {
        let result = tokio::select! {
            biased;
            _ = &mut cancelled => return Outcome::Cancelled,
            result = attempt() => result,
        };
        let last_failure = match result {
            Ok(ready) => return Outcome::Ready(ready),
            Err(AttemptFailure::Manual(reason)) => return Outcome::Failed(reason),
            Err(AttemptFailure::Transient(reason)) => reason,
        };
        tokio::select! {
            biased;
            _ = &mut cancelled => return Outcome::Cancelled,
            event = events.recv() => {
                if event.is_none() {
                    return Outcome::Failed(last_failure);
                }
            }
        }
    }
}

struct Observations {
    events: mpsc::Receiver<Availability>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Drop for Observations {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

fn observe(
    origin: String,
    process: Arc<codeterm_process_identity::PinnedProcess>,
) -> Observations {
    let (sender, events) = mpsc::channel(2);
    let ready = sender.clone();
    let readiness = tokio::spawn(async move {
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .build();
        if let Ok(client) = client {
            if super::readiness::wait(&client, &origin).await.is_ok() {
                let _ = ready.send(Availability::ServerReady).await;
            }
        }
    });
    let exited = tokio::spawn(async move {
        let result = tokio::task::spawn_blocking(move || process.wait_exit(Duration::from_secs(30)))
            .await;
        if matches!(result, Ok(Ok(()))) {
            let _ = sender.send(Availability::ExactProcessExited).await;
        }
    });
    Observations {
        events,
        tasks: vec![readiness, exited],
    }
}

pub(crate) fn cancel(app: &codeterm_client_shell::app_runtime::AppHandle) {
    if let Some(supervisor) = app.try_state::<ShellSupervisor>() {
        supervisor.reconnect.cancel();
    }
}

pub(super) async fn cancel_and_wait(
    app: &codeterm_client_shell::app_runtime::AppHandle,
) -> Result<(), String> {
    let pending = app
        .try_state::<ShellSupervisor>()
        .and_then(|supervisor| supervisor.reconnect.cancel_pending());
    let Some(mut completed) = pending else {
        return Ok(());
    };
    tokio::time::timeout(Duration::from_secs(10), async move {
        loop {
            if *completed.borrow_and_update() {
                return Ok(());
            }
            completed.changed().await
                .map_err(|_| "Recovery owner completion unavailable".to_string())?;
        }
    })
    .await
    .map_err(|_| "Recovery cancellation exceeded its failure deadline".to_string())?
}

pub(super) fn start(
    app: codeterm_client_shell::app_runtime::AppHandle,
    ready: codeterm_types::server_bootstrap::ServerReady,
    process: Arc<codeterm_process_identity::PinnedProcess>,
) {
    let lease = app.state::<ShellSupervisor>().reconnect.begin();
    let exit = crate::crash::exit::subscribe_client_exit();
    tauri::async_runtime::spawn(async move {
        let Ok(origin) = loopback_origin(ready.api_address) else {
            return;
        };
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        let mut observations = observe(origin, process);
        let attempt_app = app.clone();
        let operation = recover(&mut observations.events, lease.cancelled(exit), move || {
            let app = attempt_app.clone();
            async move {
                if crate::crash::exit::is_client_exit_pending() {
                    return Err(AttemptFailure::Manual("Domios is quitting".into()));
                }
                match crate::host_ports::native_client::retry_initialize_client(app).await {
                    Ok(initialized)
                        if initialized.native
                            == crate::host_ports::native_client::NativeClientStatus::Attached =>
                    {
                        Ok(initialized)
                    }
                    Ok(crate::host_ports::native_client::NativeClientInitialization {
                        native: crate::host_ports::native_client::NativeClientStatus::NativeHostUnavailable {
                            message, retryable,
                        },
                        ..
                    }) => {
                        if retryable {
                            Err(AttemptFailure::Transient(message))
                        } else {
                            Err(AttemptFailure::Manual(message))
                        }
                    },
                    Ok(_) => Err(AttemptFailure::Manual(
                        "Native attachment did not complete".into(),
                    )),
                    Err(error) if error.retryable() => {
                        Err(AttemptFailure::Transient(error.message().into()))
                    }
                    Err(error) => Err(AttemptFailure::Manual(error.message().into())),
                }
            }
        });
        let outcome = tokio::time::timeout_at(deadline, operation).await;
        if !lease.current() || crate::crash::exit::is_client_exit_pending() {
            return;
        }
        let failure = match outcome {
            Ok(Outcome::Ready(initialized)) => {
                tokio::select! {
                    biased;
                    _ = lease.cancelled(crate::crash::exit::subscribe_client_exit()) => None,
                    result = tokio::time::timeout_at(deadline, recover_for_attachment(app.clone(), &initialized.ready)) => {
                        match result {
                            Ok(result) => result.err(),
                            Err(_) => Some("Frontend recovery exceeded its failure deadline".into()),
                        }
                    },
                }
            }
            Ok(Outcome::Failed(reason)) => Some(reason),
            Ok(Outcome::Cancelled) => None,
            Err(_) => Some("Server recovery exceeded its failure deadline. Retry when the server is available.".into()),
        };
        if let Some(reason) = failure {
            let identity = lease.identity.clone();
            let result = run_on_main_thread(app.clone(), move |handle| {
                if handle.state::<ShellSupervisor>().reconnect.owns(&identity)
                    && !crate::crash::exit::is_client_exit_pending()
                {
                    if let Some(window) = handle.get_webview_window("starting")
                        .or_else(|| handle.get_webview_window("main"))
                    {
                        show_start_failure(&window, &reason);
                    }
                }
                Ok(())
            }).await;
            if let Err(error) = result {
                log::error!("[frontend-shell] recovery failure surface unavailable: {error}");
            }
        }
    });
}

#[cfg(test)]
mod tests;
