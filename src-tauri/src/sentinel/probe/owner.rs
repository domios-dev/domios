use std::sync::{Arc, Condvar, Mutex};
use std::thread;

#[cfg(windows)]
use super::process::{ProbeFailure, ResourceKind};
#[cfg(windows)]
use super::windows::WindowsRun;

pub(in crate::sentinel) const MAX_RESOURCE_SLOTS: usize = 8;
const MAX_DIAGNOSTICS: usize = 16;
const MAX_DIAGNOSTIC_BYTES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Free,
    Reserved,
    Active,
    Unconfirmed,
    Settled,
}

struct Slot {
    phase: Phase,
    resources: usize,
    retained: Option<Box<dyn RetainedResources>>,
    in_owner: bool,
    terminal_unconfirmed: bool,
    cancellation: Option<super::cancellation::Cancellation>,
}

impl Default for Slot {
    fn default() -> Self {
        Self {
            phase: Phase::Free,
            resources: 0,
            retained: None,
            in_owner: false,
            terminal_unconfirmed: false,
            cancellation: None,
        }
    }
}

struct State {
    slots: [Slot; MAX_RESOURCE_SLOTS],
    closed: bool,
    diagnostics: Vec<String>,
    settled_total: u64,
}

struct Core {
    state: Mutex<State>,
    changed: Condvar,
}

pub(super) trait RetainedResources: Send {
    fn request_cancel(&mut self) -> Vec<String>;
    fn wait_for_settlement(&mut self) -> Settlement;
}

pub(super) struct Settlement {
    pub primary: Option<String>,
    pub errors: Vec<String>,
    pub remains_unconfirmed: bool,
}

#[cfg(windows)]
pub(super) struct WindowsPending {
    pub run: WindowsRun,
}

#[cfg(windows)]
impl RetainedResources for WindowsPending {
    fn request_cancel(&mut self) -> Vec<String> {
        let mut failure = ProbeFailure::new("probe_cleanup");
        self.run.request_cancel(&mut failure);
        failure.cleanup_errors
    }

    fn wait_for_settlement(&mut self) -> Settlement {
        while settlement_event_required(
            self.run.failure_resources_settled(),
            self.run.permanent_unconfirmed,
            self.run.unknown_completion,
        ) {
            if let Err(error) = self.run.wait_event(None) {
                self.run.permanent_unconfirmed = true;
                self.run.unknown_completion = true;
                self.run.record_retained_cleanup_error(error);
                break;
            }
            let mut cleanup = ProbeFailure::new("probe_cleanup");
            self.run.settle_cancelled_reads(&mut cleanup);
            self.run.record_retained_cleanup_failure(cleanup);
            self.run.settle_idle_buffers();
            if let Err(error) = self.run.close_completed_runtime_handles() {
                self.run.permanent_unconfirmed = true;
                self.run.record_retained_cleanup_error(error);
                break;
            }
        }
        let failure = self
            .run
            .failure
            .take()
            .unwrap_or_else(|| ProbeFailure::new("probe_cleanup"));
        Settlement {
            primary: Some(failure.primary.to_string()),
            errors: failure.cleanup_errors,
            remains_unconfirmed: !self.run.failure_resources_settled()
                || self.run.permanent_unconfirmed
                || self.run.unknown_completion,
        }
    }
}

#[cfg(windows)]
impl WindowsRun {
    pub(super) fn close_reader(&mut self, stdout: bool) -> Result<(), String> {
        if self.stream(stdout).pending_active || self.stream(stdout).connect_pending {
            return Err("probe read endpoint cannot close while I/O is pending".into());
        }
        if let Err(error) = self.stream_mut(stdout).reader.close() {
            return Err(mark_close_uncertain(&mut self.permanent_unconfirmed, error));
        }
        let capture_registered =
            std::mem::replace(&mut self.stream_mut(stdout).capture_registered, false);
        self.settle_resource(ResourceKind::Capture, capture_registered);
        let buffer_registered =
            std::mem::replace(&mut self.stream_mut(stdout).buffer_registered, false);
        self.settle_resource(ResourceKind::OverlappedIo, buffer_registered);
        let connect_buffer_registered = std::mem::replace(
            &mut self.stream_mut(stdout).connect_buffer_registered,
            false,
        );
        self.settle_resource(ResourceKind::OverlappedIo, connect_buffer_registered);
        Ok(())
    }

    pub(super) fn settle_cancelled_reads(&mut self, failure: &mut ProbeFailure) {
        for stdout in [true, false] {
            if !self.stream(stdout).pending_active && !self.stream(stdout).connect_pending {
                if (self.stream(stdout).read_failed
                    || self.failure.is_some()
                    || self.cancel_attempted)
                    && self.stream(stdout).reader.is_open()
                {
                    if let Err(error) = self.close_reader(stdout) {
                        failure.cleanup_error(error);
                        self.permanent_unconfirmed = true;
                    }
                }
                continue;
            }
            if !self.stream(stdout).reader.is_open() {
                self.unknown_completion = true;
                self.permanent_unconfirmed = true;
                failure.unconfirmed(ResourceKind::OverlappedIo);
            }
        }
    }

    pub(super) fn settle_idle_buffers(&mut self) {
        for stdout in [true, false] {
            if !self.stream(stdout).connect_pending {
                let registered = std::mem::replace(
                    &mut self.stream_mut(stdout).connect_buffer_registered,
                    false,
                );
                self.settle_resource(ResourceKind::OverlappedIo, registered);
            }
            if !self.stream(stdout).pending_active && !self.stream(stdout).reader.is_open() {
                let registered =
                    std::mem::replace(&mut self.stream_mut(stdout).buffer_registered, false);
                self.settle_resource(ResourceKind::OverlappedIo, registered);
            }
        }
    }

    pub(super) fn close_completed_runtime_handles(&mut self) -> Result<(), String> {
        if self.parent_exited && self.job_empty {
            if let Err(error) = self.job.close() {
                return Err(mark_close_uncertain(&mut self.permanent_unconfirmed, error));
            }
            let registered = std::mem::replace(&mut self.job_registered, false);
            self.settle_resource(ResourceKind::Job, registered);
        }
        if !self.job.is_open()
            && !self.stdout.connect_pending
            && !self.stderr.connect_pending
            && !self.stdout.pending_active
            && !self.stderr.pending_active
            && !self.stdout.reader.is_open()
            && !self.stderr.reader.is_open()
        {
            self.detach_cancel_port();
            if let Err(error) = self.port.close() {
                return Err(mark_close_uncertain(&mut self.permanent_unconfirmed, error));
            }
            let registered = std::mem::replace(&mut self.port_registered, false);
            self.settle_resource(ResourceKind::OverlappedIo, registered);
        }
        if !self.port.is_open() && !self.process.is_open() {
            if let Err(error) = self.close_cancellation() {
                return Err(mark_close_uncertain(&mut self.permanent_unconfirmed, error));
            }
        }
        Ok(())
    }

    pub(super) fn runtime_success_ready(&self) -> bool {
        self.parent_exited
            && self.job_empty
            && self.stdout.eof
            && self.stderr.eof
            && !self.stdout.pending_active
            && !self.stderr.pending_active
            && !self.job.is_open()
            && !self.port.is_open()
            && self.events.is_none()
            && self.failure.is_none()
    }

    pub(super) fn close_completed_handles(&mut self) -> Result<(), String> {
        if !self.runtime_success_ready() {
            return Err("probe success facts are incomplete".into());
        }
        if let Err(error) = self.stdout.reader.close() {
            return Err(mark_close_uncertain(&mut self.permanent_unconfirmed, error));
        }
        let stdout_registered = std::mem::replace(&mut self.stdout.capture_registered, false);
        self.settle_resource(ResourceKind::Capture, stdout_registered);
        if let Err(error) = self.stderr.reader.close() {
            return Err(mark_close_uncertain(&mut self.permanent_unconfirmed, error));
        }
        let stderr_registered = std::mem::replace(&mut self.stderr.capture_registered, false);
        self.settle_resource(ResourceKind::Capture, stderr_registered);
        if let Err(error) = self.job.close() {
            return Err(mark_close_uncertain(&mut self.permanent_unconfirmed, error));
        }
        let job_registered = std::mem::replace(&mut self.job_registered, false);
        self.settle_resource(ResourceKind::Job, job_registered);
        self.detach_cancel_port();
        if let Err(error) = self.port.close() {
            return Err(mark_close_uncertain(&mut self.permanent_unconfirmed, error));
        }
        let port_registered = std::mem::replace(&mut self.port_registered, false);
        self.settle_resource(ResourceKind::OverlappedIo, port_registered);
        self.close_cancellation()
            .map_err(|error| mark_close_uncertain(&mut self.permanent_unconfirmed, error))
    }

    pub(super) fn failure_resources_settled(&self) -> bool {
        !self.process.is_open()
            && (!self.assigned || (self.parent_exited && self.job_empty))
            && !self.primary_thread.is_open()
            && !self.stdin.is_open()
            && !self.stdout.child_writer.is_open()
            && !self.stderr.child_writer.is_open()
            && !self.stdout.reader.is_open()
            && !self.stderr.reader.is_open()
            && !self.stdout.pending_active
            && !self.stderr.pending_active
            && !self.stdout.connect_pending
            && !self.stderr.connect_pending
            && !self.job.is_open()
            && !self.port.is_open()
            && self.events.is_none()
    }

    pub(super) fn owner_is_closing(&self) -> bool {
        self.owner
            .upgrade()
            .map_or(true, |owner| owner.is_closing())
    }

    pub(super) fn record_owner_failure(&self, diagnostic: impl AsRef<str>) {
        if let Some(owner) = self.owner.upgrade() {
            owner.record_failure(diagnostic);
        }
    }

    pub(super) fn record_retained_cleanup_failure(&mut self, cleanup: ProbeFailure) {
        if cleanup.cleanup_errors.is_empty() && cleanup.unconfirmed.is_empty() {
            return;
        }
        let failure = self
            .failure
            .get_or_insert_with(|| ProbeFailure::new("probe_cleanup"));
        failure.record_cleanup_errors(cleanup.cleanup_errors);
        for resource in cleanup.unconfirmed {
            failure.unconfirmed(resource);
        }
    }

    pub(super) fn record_retained_cleanup_error(&mut self, error: impl AsRef<str>) {
        self.failure
            .get_or_insert_with(|| ProbeFailure::new("probe_cleanup"))
            .cleanup_error(error);
    }
}

pub(super) fn settlement_event_required(
    resources_settled: bool,
    permanent_unconfirmed: bool,
    unknown_completion: bool,
) -> bool {
    !resources_settled && !permanent_unconfirmed && !unknown_completion
}

pub(super) fn mark_close_uncertain(permanent: &mut bool, error: String) -> String {
    *permanent = true;
    error
}

fn settlement_diagnostics(core: &Core, settlement: Settlement) {
    if let Some(primary) = settlement.primary {
        push_diagnostic(core, &primary);
    }
    for error in settlement.errors {
        push_diagnostic(core, &error);
    }
}

pub(in crate::sentinel) struct ProbeResourceOwner {
    core: Arc<Core>,
    unavailable: Option<String>,
}

pub(super) struct Reservation {
    core: Arc<Core>,
    index: usize,
    resource_count: usize,
    finished: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum AdmissionError {
    Full,
    Closed,
    Unavailable(String),
}

#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
pub(in crate::sentinel) struct Snapshot {
    pub free: usize,
    pub reserved: usize,
    pub active: usize,
    pub unconfirmed: usize,
    pub settled_total: u64,
    pub diagnostics: Vec<String>,
}

impl ProbeResourceOwner {
    pub(in crate::sentinel) fn new() -> Self {
        let core = Arc::new(Core {
            state: Mutex::new(State {
                slots: std::array::from_fn(|_| Slot::default()),
                closed: false,
                diagnostics: Vec::new(),
                settled_total: 0,
            }),
            changed: Condvar::new(),
        });
        let mut unavailable = None;
        for index in 0..MAX_RESOURCE_SLOTS {
            let worker_core = core.clone();
            match thread::Builder::new()
                .name(format!("sentinel-probe-owner-{index}"))
                .spawn(move || owner_loop(worker_core, index))
            {
                Ok(worker) => drop(worker),
                Err(error) => {
                    unavailable = Some(error.to_string());
                    close_core(&core);
                    break;
                }
            }
        }
        Self { core, unavailable }
    }

    pub(super) fn reserve(&self) -> Result<Reservation, AdmissionError> {
        if let Some(error) = &self.unavailable {
            return Err(AdmissionError::Unavailable(error.clone()));
        }
        let mut state = self
            .core
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.closed {
            return Err(AdmissionError::Closed);
        }
        let Some(index) = state
            .slots
            .iter()
            .position(|slot| slot.phase == Phase::Free)
        else {
            return Err(AdmissionError::Full);
        };
        state.slots[index] = Slot {
            phase: Phase::Reserved,
            ..Slot::default()
        };
        self.core.changed.notify_all();
        Ok(Reservation {
            core: self.core.clone(),
            index,
            resource_count: 0,
            finished: false,
        })
    }

    pub(in crate::sentinel) fn close_admission(&self) {
        close_core(&self.core);
    }

    pub(in crate::sentinel) fn is_closing(&self) -> bool {
        self.core
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .closed
    }

    pub(super) fn record_failure(&self, diagnostic: impl AsRef<str>) {
        push_diagnostic(&self.core, diagnostic.as_ref());
    }

    #[cfg(test)]
    pub(in crate::sentinel) fn snapshot(&self) -> Snapshot {
        let state = self
            .core
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        Snapshot {
            free: state
                .slots
                .iter()
                .filter(|slot| slot.phase == Phase::Free)
                .count(),
            reserved: state
                .slots
                .iter()
                .filter(|slot| slot.phase == Phase::Reserved)
                .count(),
            active: state
                .slots
                .iter()
                .filter(|slot| slot.phase == Phase::Active)
                .count(),
            unconfirmed: state
                .slots
                .iter()
                .filter(|slot| slot.phase == Phase::Unconfirmed)
                .count(),
            settled_total: state.settled_total,
            diagnostics: state.diagnostics.clone(),
        }
    }

    #[cfg(test)]
    pub(super) fn wait_for_unconfirmed_with_timeout(&self, timeout: std::time::Duration) -> bool {
        self.wait_for_unconfirmed_state(timeout, false)
    }

    #[cfg(test)]
    pub(super) fn wait_for_terminal_unconfirmed_with_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> bool {
        self.wait_for_unconfirmed_state(timeout, true)
    }

    #[cfg(test)]
    fn wait_for_unconfirmed_state(&self, timeout: std::time::Duration, terminal: bool) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        let mut state = self
            .core
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        loop {
            if state.slots.iter().any(|slot| {
                slot.phase == Phase::Unconfirmed && (!terminal || slot.terminal_unconfirmed)
            }) {
                return true;
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return false;
            }
            let (next, result) = self
                .core
                .changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(|error| error.into_inner());
            state = next;
            if result.timed_out()
                && !state.slots.iter().any(|slot| {
                    slot.phase == Phase::Unconfirmed && (!terminal || slot.terminal_unconfirmed)
                })
            {
                return false;
            }
        }
    }

    #[cfg(test)]
    pub(super) fn wait_for_free_slots_with_timeout(
        &self,
        expected: usize,
        timeout: std::time::Duration,
    ) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        let mut state = self
            .core
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        loop {
            if state
                .slots
                .iter()
                .filter(|slot| slot.phase == Phase::Free)
                .count()
                >= expected
            {
                return true;
            }
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return false;
            }
            let (next, result) = self
                .core
                .changed
                .wait_timeout(state, remaining)
                .unwrap_or_else(|error| error.into_inner());
            state = next;
            if result.timed_out()
                && state
                    .slots
                    .iter()
                    .filter(|slot| slot.phase == Phase::Free)
                    .count()
                    < expected
            {
                return false;
            }
        }
    }
}

impl Reservation {
    pub(super) fn register_cancellation(
        &mut self,
        cancellation: super::cancellation::Cancellation,
    ) -> Result<(), super::process::ProbeFailure> {
        let mut state = self
            .core
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.closed {
            cancellation.cancel();
            return Err(super::process::ProbeFailure::new("probe_owner_closing"));
        }
        state.slots[self.index].cancellation = Some(cancellation);
        Ok(())
    }

    pub(super) fn register_resource(&mut self) -> Result<(), String> {
        let mut state = self
            .core
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let slot = &mut state.slots[self.index];
        if !matches!(slot.phase, Phase::Reserved | Phase::Active) {
            return Err("probe resource registered outside an owned slot".into());
        }
        slot.phase = Phase::Active;
        slot.resources = slot.resources.saturating_add(1);
        self.resource_count = self.resource_count.saturating_add(1);
        self.core.changed.notify_all();
        Ok(())
    }

    pub(super) fn resource_settled(&mut self) -> Result<(), String> {
        let mut state = self
            .core
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let slot = &mut state.slots[self.index];
        if self.resource_count == 0 || slot.resources == 0 {
            return Err("probe resource settlement was not registered".into());
        }
        slot.resources -= 1;
        self.resource_count -= 1;
        self.core.changed.notify_all();
        Ok(())
    }

    pub(super) fn release(mut self) -> Result<(), String> {
        if self.resource_count != 0 {
            return Err("probe slot released with unsettled resources".into());
        }
        let mut state = self
            .core
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let slot = &mut state.slots[self.index];
        if slot.resources != 0 || !matches!(slot.phase, Phase::Reserved | Phase::Active) {
            return Err("probe slot ownership changed before release".into());
        }
        *slot = Slot::default();
        self.finished = true;
        self.core.changed.notify_all();
        Ok(())
    }

    pub(super) fn retain(mut self, resources: Box<dyn RetainedResources>) {
        let mut state = self
            .core
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let slot = &mut state.slots[self.index];
        slot.phase = Phase::Unconfirmed;
        slot.retained = Some(resources);
        self.finished = true;
        self.core.changed.notify_all();
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let mut state = self
            .core
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let slot = &mut state.slots[self.index];
        if slot.phase == Phase::Reserved && slot.resources == 0 {
            *slot = Slot::default();
        } else {
            slot.phase = Phase::Unconfirmed;
            push_diagnostic_locked(&mut state, "probe reservation dropped with owned resources");
        }
        self.core.changed.notify_all();
    }
}

impl Drop for ProbeResourceOwner {
    fn drop(&mut self) {
        self.close_admission();
    }
}

fn owner_loop(core: Arc<Core>, index: usize) {
    loop {
        let mut resources = {
            let mut state = core.state.lock().unwrap_or_else(|error| error.into_inner());
            loop {
                let resources = {
                    let slot = &mut state.slots[index];
                    if slot.retained.is_some() && !slot.in_owner && !slot.terminal_unconfirmed {
                        slot.in_owner = true;
                        slot.retained.take()
                    } else {
                        None
                    }
                };
                if let Some(resources) = resources {
                    break resources;
                }
                if state.closed && state.slots[index].phase == Phase::Free {
                    return;
                }
                if state.slots[index].terminal_unconfirmed {
                    state = core
                        .changed
                        .wait(state)
                        .unwrap_or_else(|error| error.into_inner());
                    continue;
                }
                state = core
                    .changed
                    .wait(state)
                    .unwrap_or_else(|error| error.into_inner());
            }
        };
        let cancel_errors = resources.request_cancel();
        let settlement = resources.wait_for_settlement();
        for error in cancel_errors {
            push_diagnostic(&core, &error);
        }
        let remains_unconfirmed = settlement.remains_unconfirmed;
        settlement_diagnostics(&core, settlement);
        if remains_unconfirmed {
            let mut state = core.state.lock().unwrap_or_else(|error| error.into_inner());
            state.slots[index].retained = Some(resources);
            state.slots[index].in_owner = false;
            state.slots[index].terminal_unconfirmed = true;
            state.slots[index].phase = Phase::Unconfirmed;
            core.changed.notify_all();
            continue;
        } else {
            {
                let mut state = core.state.lock().unwrap_or_else(|error| error.into_inner());
                state.slots[index].phase = Phase::Settled;
                state.settled_total = state.settled_total.saturating_add(1);
            }
            drop(resources);
            let mut state = core.state.lock().unwrap_or_else(|error| error.into_inner());
            state.slots[index] = Slot::default();
            core.changed.notify_all();
        }
        let state = core.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.closed && state.slots[index].phase == Phase::Free {
            return;
        }
    }
}

fn close_core(core: &Core) {
    let mut state = core.state.lock().unwrap_or_else(|error| error.into_inner());
    state.closed = true;
    for slot in &state.slots {
        if let Some(cancellation) = &slot.cancellation {
            cancellation.cancel();
        }
    }
    core.changed.notify_all();
}

fn push_diagnostic(core: &Core, diagnostic: &str) {
    let mut state = core.state.lock().unwrap_or_else(|error| error.into_inner());
    push_diagnostic_locked(&mut state, diagnostic);
    core.changed.notify_all();
}

fn push_diagnostic_locked(state: &mut State, diagnostic: &str) {
    if state.diagnostics.len() == MAX_DIAGNOSTICS {
        state.diagnostics.remove(0);
    }
    state
        .diagnostics
        .push(diagnostic.chars().take(MAX_DIAGNOSTIC_BYTES).collect());
}
