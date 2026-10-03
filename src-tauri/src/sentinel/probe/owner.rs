use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

#[cfg(windows)]
use super::process::{ProbeFailure, ResourceKind};
#[cfg(windows)]
use super::windows::WindowsRun;

pub(super) const MAX_RESOURCE_SLOTS: usize = 8;
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
    settlement_attempted: bool,
}

impl Default for Slot {
    fn default() -> Self {
        Self {
            phase: Phase::Free,
            resources: 0,
            retained: None,
            in_owner: false,
            settlement_attempted: false,
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
        while !self.run.failure_resources_settled() && !self.run.unknown_completion {
            if let Err(error) = self.run.wait_event(None) {
                self.run.permanent_unconfirmed = true;
                self.run.unknown_completion = true;
                self.run.owner.record_failure(error);
                break;
            }
            let mut ignored = ProbeFailure::new("probe_cleanup");
            self.run.settle_cancelled_reads(&mut ignored);
            self.run.settle_idle_buffers();
            if let Err(error) = self.run.close_completed_runtime_handles() {
                self.run.permanent_unconfirmed = true;
                self.run.owner.record_failure(error);
                break;
            }
        }
        let errors = self
            .run
            .failure
            .take()
            .map(|failure| failure.cleanup_errors)
            .unwrap_or_default();
        Settlement {
            errors,
            remains_unconfirmed: !self.run.failure_resources_settled()
                || self.run.permanent_unconfirmed
                || self.run.unknown_completion,
        }
    }
}

#[cfg(windows)]
impl WindowsRun {
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
            self.job.close()?;
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
            self.port.close()?;
            let registered = std::mem::replace(&mut self.port_registered, false);
            self.settle_resource(ResourceKind::OverlappedIo, registered);
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
            && self.failure.is_none()
    }

    pub(super) fn close_completed_handles(&mut self) -> Result<(), String> {
        if !self.runtime_success_ready() {
            return Err("probe success facts are incomplete".into());
        }
        self.stdout.reader.close()?;
        let stdout_registered = std::mem::replace(&mut self.stdout.capture_registered, false);
        self.settle_resource(ResourceKind::Capture, stdout_registered);
        self.stderr.reader.close()?;
        let stderr_registered = std::mem::replace(&mut self.stderr.capture_registered, false);
        self.settle_resource(ResourceKind::Capture, stderr_registered);
        self.job.close()?;
        let job_registered = std::mem::replace(&mut self.job_registered, false);
        self.settle_resource(ResourceKind::Job, job_registered);
        self.port.close()?;
        let port_registered = std::mem::replace(&mut self.port_registered, false);
        self.settle_resource(ResourceKind::OverlappedIo, port_registered);
        Ok(())
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
    }
}

pub(in crate::sentinel) struct ProbeResourceOwner {
    core: Arc<Core>,
    worker: Mutex<Option<JoinHandle<()>>>,
    unavailable: Option<String>,
}

pub(super) struct Reservation {
    owner: Arc<ProbeResourceOwner>,
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
pub(super) struct Snapshot {
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
        let worker_core = core.clone();
        let worker = thread::Builder::new()
            .name("sentinel-probe-owner".into())
            .spawn(move || owner_loop(worker_core));
        let (worker, unavailable) = match worker {
            Ok(worker) => (Some(worker), None),
            Err(error) => (None, Some(error.to_string())),
        };
        Self {
            core,
            worker: Mutex::new(worker),
            unavailable,
        }
    }

    pub(super) fn reserve(self: &Arc<Self>) -> Result<Reservation, AdmissionError> {
        if let Some(error) = &self.unavailable {
            return Err(AdmissionError::Unavailable(error.clone()));
        }
        let mut state = self.core.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.closed {
            return Err(AdmissionError::Closed);
        }
        let Some(index) = state.slots.iter().position(|slot| slot.phase == Phase::Free) else {
            return Err(AdmissionError::Full);
        };
        state.slots[index] = Slot {
            phase: Phase::Reserved,
            ..Slot::default()
        };
        self.core.changed.notify_all();
        Ok(Reservation {
            owner: self.clone(),
            index,
            resource_count: 0,
            finished: false,
        })
    }

    pub(super) fn close_admission(&self) {
        let mut state = self.core.state.lock().unwrap_or_else(|error| error.into_inner());
        state.closed = true;
        self.core.changed.notify_all();
    }

    pub(super) fn is_closing(&self) -> bool {
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
    pub(super) fn snapshot(&self) -> Snapshot {
        let state = self.core.state.lock().unwrap_or_else(|error| error.into_inner());
        Snapshot {
            free: state.slots.iter().filter(|slot| slot.phase == Phase::Free).count(),
            reserved: state
                .slots
                .iter()
                .filter(|slot| slot.phase == Phase::Reserved)
                .count(),
            active: state.slots.iter().filter(|slot| slot.phase == Phase::Active).count(),
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
    pub(super) fn wait_for_unconfirmed(&self) {
        let mut state = self.core.state.lock().unwrap_or_else(|error| error.into_inner());
        while !state.slots.iter().any(|slot| slot.phase == Phase::Unconfirmed) {
            state = self
                .core
                .changed
                .wait(state)
                .unwrap_or_else(|error| error.into_inner());
        }
    }

    #[cfg(test)]
    pub(super) fn wait_for_free_slots(&self, expected: usize) {
        let mut state = self.core.state.lock().unwrap_or_else(|error| error.into_inner());
        while state.slots.iter().filter(|slot| slot.phase == Phase::Free).count() < expected {
            state = self
                .core
                .changed
                .wait(state)
                .unwrap_or_else(|error| error.into_inner());
        }
    }

    #[cfg(test)]
    pub(super) fn wait_for_free_slots_with_timeout(
        &self,
        expected: usize,
        timeout: std::time::Duration,
    ) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        let mut state = self.core.state.lock().unwrap_or_else(|error| error.into_inner());
        loop {
            if state.slots.iter().filter(|slot| slot.phase == Phase::Free).count() >= expected {
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
                && state.slots.iter().filter(|slot| slot.phase == Phase::Free).count() < expected
            {
                return false;
            }
        }
    }
}

impl Reservation {
    pub(super) fn register_resource(&mut self) -> Result<(), String> {
        let mut state = self
            .owner
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
        self.owner.core.changed.notify_all();
        Ok(())
    }

    pub(super) fn resource_settled(&mut self) -> Result<(), String> {
        let mut state = self
            .owner
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
        self.owner.core.changed.notify_all();
        Ok(())
    }

    pub(super) fn release(mut self) -> Result<(), String> {
        if self.resource_count != 0 {
            return Err("probe slot released with unsettled resources".into());
        }
        let mut state = self
            .owner
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
        self.owner.core.changed.notify_all();
        Ok(())
    }

    pub(super) fn retain(mut self, resources: Box<dyn RetainedResources>) {
        let mut state = self
            .owner
            .core
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let slot = &mut state.slots[self.index];
        slot.phase = Phase::Unconfirmed;
        slot.retained = Some(resources);
        self.finished = true;
        self.owner.core.changed.notify_all();
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let mut state = self
            .owner
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
        self.owner.core.changed.notify_all();
    }
}

impl Drop for ProbeResourceOwner {
    fn drop(&mut self) {
        self.close_admission();
        if let Ok(worker) = self.worker.get_mut() {
            drop(worker.take());
        }
    }
}

fn owner_loop(core: Arc<Core>) {
    loop {
        let (index, mut resources) = {
            let mut state = core.state.lock().unwrap_or_else(|error| error.into_inner());
            loop {
                if let Some(index) = state.slots.iter().position(|slot| {
                    slot.retained.is_some() && !slot.settlement_attempted && !slot.in_owner
                }) {
                    let slot = &mut state.slots[index];
                    slot.in_owner = true;
                    if let Some(resources) = slot.retained.take() {
                        break (index, resources);
                    }
                }
                if state.closed && state.slots.iter().all(|slot| slot.phase == Phase::Free) {
                    return;
                }
                state = core
                    .changed
                    .wait(state)
                    .unwrap_or_else(|error| error.into_inner());
            }
        };
        let cancel_errors = resources.request_cancel();
        let settlement = resources.wait_for_settlement();
        for error in cancel_errors.into_iter().chain(settlement.errors) {
            push_diagnostic(&core, &error);
        }
        if settlement.remains_unconfirmed {
            let mut state = core.state.lock().unwrap_or_else(|error| error.into_inner());
            state.slots[index].retained = Some(resources);
            state.slots[index].in_owner = false;
            state.slots[index].settlement_attempted = true;
            state.slots[index].phase = Phase::Unconfirmed;
            core.changed.notify_all();
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
        let closed_and_empty = {
            let state = core.state.lock().unwrap_or_else(|error| error.into_inner());
            state.closed && state.slots.iter().all(|slot| slot.phase == Phase::Free)
        };
        if closed_and_empty {
            return;
        }
    }
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
