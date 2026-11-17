use super::*;
use tokio::sync::watch;

pub(super) struct RecoveryOwner {
    current: watch::Sender<Option<Arc<()>>>,
    completion: parking_lot::Mutex<Option<watch::Receiver<bool>>>,
    incident: parking_lot::Mutex<Incident>,
    root: watch::Sender<RootOutcome>,
}

impl Default for RecoveryOwner {
    fn default() -> Self {
        Self {
            current: watch::channel(None).0,
            completion: parking_lot::Mutex::new(None),
            incident: Default::default(),
            root: watch::channel(RootOutcome::Pending).0,
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
        self.current
            .borrow()
            .as_ref()
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

const MAX_AUTOMATIC_ATTEMPTS: u8 = 2;

#[derive(Default)]
struct Incident {
    running: bool,
    exhausted: bool,
    attempt: u8,
    after_navigation: u64,
}

#[derive(Clone)]
enum RootOutcome {
    Pending,
    Ready,
    Failed(FrontendFailure),
}

#[derive(Debug, PartialEq, Eq)]
enum Decision {
    Recover,
    Join,
    ShowFailure,
}

impl Incident {
    fn failure(&mut self, code: FailureCode) -> Decision {
        if self.running {
            Decision::Join
        } else if self.exhausted || !code.recoverable() {
            Decision::ShowFailure
        } else {
            self.running = true;
            Decision::Recover
        }
    }

    fn accepts_root(&self, navigation: u64) -> bool {
        self.running && navigation > self.after_navigation
    }

    fn next_attempt(&mut self, retryable: bool) -> Option<u8> {
        if !retryable || self.attempt >= MAX_AUTOMATIC_ATTEMPTS {
            self.running = false;
            self.exhausted = true;
            return None;
        }
        self.attempt += 1;
        Some(self.attempt)
    }
}

pub(super) fn recover_before_failure(
    app: &codeterm_client_shell::app_runtime::AppHandle,
    code: FailureCode,
    reason: &str,
) -> bool {
    if crate::crash::exit::is_client_exit_pending() {
        return true;
    }
    let Some(supervisor) = app.try_state::<ShellSupervisor>() else {
        return false;
    };
    let owner = &supervisor.reconnect;
    let mut incident = owner.incident.lock();
    match incident.failure(code) {
        Decision::ShowFailure => false,
        Decision::Join => {
            owner
                .root
                .send_replace(RootOutcome::Failed(FrontendFailure::new(code, reason)));
            true
        }
        Decision::Recover => {
            let lease = owner.begin();
            drop(incident);
            let app = app.clone();
            let failure = FrontendFailure::new(code, reason);
            tauri::async_runtime::spawn(run(app, lease, failure));
            true
        }
    }
}

async fn root_outcome(events: &mut watch::Receiver<RootOutcome>) -> Result<(), FrontendFailure> {
    loop {
        match events.borrow_and_update().clone() {
            RootOutcome::Ready => return Ok(()),
            RootOutcome::Failed(failure) => return Err(failure),
            RootOutcome::Pending => {}
        }
        events.changed().await.map_err(|_| {
            FrontendFailure::new(
                FailureCode::ReconnectFailed,
                "Root acknowledgement channel closed",
            )
        })?;
    }
}

async fn run(
    app: codeterm_client_shell::app_runtime::AppHandle,
    lease: Lease,
    mut failure: FrontendFailure,
) {
    let mut events = app.state::<ShellSupervisor>().reconnect.root.subscribe();
    loop {
        if !lease.current() || crate::crash::exit::is_client_exit_pending() {
            return;
        }
        let attempt = {
            let supervisor = app.state::<ShellSupervisor>();
            let mut incident = supervisor.reconnect.incident.lock();
            incident.after_navigation = supervisor.navigation.load(Ordering::Acquire);
            incident.next_attempt(failure.code.recoverable())
        };
        let Some(attempt) = attempt else {
            break;
        };
        app.state::<ShellSupervisor>()
            .reconnect
            .root
            .send_replace(RootOutcome::Pending);
        failure
            .code
            .record_attempt(attempt, "started", &failure.reason);
        let operation = async {
            run_on_main_thread(app.clone(), |handle| surfaces::show_reconnecting(&handle))
                .await
                .map_err(|reason| {
                    FailureCode::RecoveryNavigationFailed.record(&reason);
                    FrontendFailure::new(FailureCode::RecoveryNavigationFailed, reason)
                })?;
            recovery::retry_once(app.clone(), attempt == MAX_AUTOMATIC_ATTEMPTS)
                .await
                .map_err(|error| {
                    error.code.record(&error.reason);
                    error
                })?;
            root_outcome(&mut events).await
        };
        let result = tokio::select! {
            biased;
            _ = lease.cancelled(crate::crash::exit::subscribe_client_exit()) => {
                failure.code.record_attempt(attempt, "cancelled", "Recovery owner retired");
                return;
            },
            result = tokio::time::timeout(Duration::from_secs(60), operation) => {
                result.unwrap_or_else(|_| {
                    let reason = "Automatic recovery exceeded its failure deadline";
                    FailureCode::AutomaticRecoveryDeadline.record(reason);
                    Err(FrontendFailure::new(FailureCode::AutomaticRecoveryDeadline, reason))
                })
            },
        };
        if !lease.current() {
            return;
        }
        match result {
            Ok(()) => {
                failure
                    .code
                    .record_attempt(attempt, "recovered", "Root render acknowledged");
                reset(&app);
                return;
            }
            Err(error) => {
                error.code.record_attempt(attempt, "failed", &error.reason);
                failure = error;
            }
        }
    }
    let attempts = app
        .state::<ShellSupervisor>()
        .reconnect
        .incident
        .lock()
        .attempt;
    failure
        .code
        .record_attempt(attempts, "exhausted", &failure.reason);
    let detail = failure.code.detail(&failure.reason);
    let identity = lease.identity.clone();
    let _ = run_on_main_thread(app, move |handle| {
        if !handle.state::<ShellSupervisor>().reconnect.owns(&identity)
            || crate::crash::exit::is_client_exit_pending()
        {
            return Ok(());
        }
        if let Some(window) = handle
            .get_webview_window("starting")
            .or_else(|| handle.get_webview_window("main"))
        {
            surfaces::show_start_failure_recorded(&window, &detail);
        }
        Ok(())
    })
    .await;
}

pub(super) fn root_ready(
    app: &codeterm_client_shell::app_runtime::AppHandle,
    generation: ClientViewGeneration,
) {
    let supervisor = app.state::<ShellSupervisor>();
    let owner = &supervisor.reconnect;
    let incident = owner.incident.lock();
    if incident.accepts_root(generation.navigation) {
        owner.root.send_replace(RootOutcome::Ready);
    }
}

pub(super) fn reset(app: &codeterm_client_shell::app_runtime::AppHandle) {
    *app.state::<ShellSupervisor>().reconnect.incident.lock() = Incident::default();
}

pub(crate) fn cancel(app: &codeterm_client_shell::app_runtime::AppHandle) {
    if let Some(supervisor) = app.try_state::<ShellSupervisor>() {
        supervisor.reconnect.cancel();
        *supervisor.reconnect.incident.lock() = Incident::default();
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
            completed
                .changed()
                .await
                .map_err(|_| "Recovery owner completion unavailable".to_string())?;
        }
    })
    .await
    .map_err(|_| "Recovery cancellation exceeded its failure deadline".to_string())?
}

#[cfg(test)]
mod tests;
