use std::fmt;
use std::time::{Duration, Instant};

const MAX_CLEANUP_ERRORS: usize = 8;
const MAX_ERROR_BYTES: usize = 192;
const MAX_REASON_BYTES: usize = 240;

#[cfg(any(unix, test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LeaderOwnership {
    #[cfg(unix)]
    Unspawned,
    Retained(u32),
    Retired(u32),
    Lost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::sentinel) enum ResourceKind {
    Child,
    ProcessGroup,
    Capture,
    #[cfg_attr(not(windows), allow(dead_code))]
    Job,
    #[cfg_attr(not(windows), allow(dead_code))]
    OverlappedIo,
}

impl ResourceKind {
    fn name(self) -> &'static str {
        match self {
            Self::Child => "child",
            Self::ProcessGroup => "process-group",
            Self::Capture => "capture",
            Self::Job => "job",
            Self::OverlappedIo => "overlapped-io",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::sentinel) enum ResourceState {
    Confirmed,
    Unconfirmed,
}

#[derive(Debug, Clone)]
pub(in crate::sentinel) struct ProbeFailure {
    pub primary: &'static str,
    pub resources: ResourceState,
    pub processes_confirmed: bool,
    pub cleanup_errors: Vec<String>,
    pub unconfirmed: Vec<ResourceKind>,
    pub late_by: Option<Duration>,
    pub exit_code: Option<i32>,
    pub stdout: Option<crate::sentinel::command::Capture>,
    pub stderr: Option<crate::sentinel::command::Capture>,
}

impl ProbeFailure {
    pub(in crate::sentinel) fn new(primary: &'static str) -> Self {
        Self {
            primary,
            resources: ResourceState::Confirmed,
            processes_confirmed: false,
            cleanup_errors: Vec::new(),
            unconfirmed: Vec::new(),
            late_by: None,
            exit_code: None,
            stdout: None,
            stderr: None,
        }
    }

    pub(in crate::sentinel) fn with_output(mut self, output: &super::Output) -> Self {
        self.exit_code = output.exit_code;
        self.stdout = Some(output.stdout.clone());
        self.stderr = Some(output.stderr.clone());
        self
    }

    pub(in crate::sentinel) fn is_deadline(&self) -> bool {
        matches!(
            self.primary,
            "probe_timeout"
                | "probe_deadline_before_spawn"
                | "probe_cleanup_deadline"
                | "probe_deadline_unavailable"
        )
    }

    pub(super) fn cleanup_error(&mut self, error: impl AsRef<str>) {
        if self.cleanup_errors.len() == MAX_CLEANUP_ERRORS {
            return;
        }
        self.cleanup_errors
            .push(error.as_ref().chars().take(MAX_ERROR_BYTES).collect());
    }

    pub(in crate::sentinel) fn unconfirmed(&mut self, resource: ResourceKind) {
        self.resources = ResourceState::Unconfirmed;
        if !self.unconfirmed.contains(&resource) {
            self.unconfirmed.push(resource);
        }
    }

    pub(in crate::sentinel) fn is_unconfirmed(&self) -> bool {
        self.resources == ResourceState::Unconfirmed
    }

    pub(in crate::sentinel) fn reason(&self) -> String {
        let mut reason = self.primary.to_string();
        if !self.unconfirmed.is_empty() {
            reason.push_str("; unconfirmed=");
            reason.push_str(
                &self
                    .unconfirmed
                    .iter()
                    .map(|resource| resource.name())
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }
        if let Some(late_by) = self.late_by {
            reason.push_str(&format!("; late={}ms", late_by.as_millis()));
        }
        reason.chars().take(MAX_REASON_BYTES).collect()
    }

    pub(super) fn record_cleanup_errors(&mut self, errors: impl IntoIterator<Item = String>) {
        for error in errors {
            self.cleanup_error(error);
        }
    }
}

impl fmt::Display for ProbeFailure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.reason())
    }
}

impl std::error::Error for ProbeFailure {}

#[derive(Clone, Copy)]
pub(super) struct Deadlines {
    #[cfg(test)]
    pub admitted: Instant,
    pub operation: Instant,
    pub hard: Instant,
    #[cfg(test)]
    pub duration: Duration,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(super) struct DeadlineReceipt {
    pub admitted: Instant,
    pub operation: Instant,
    pub hard: Instant,
    pub duration: Duration,
}

impl Deadlines {
    pub(super) fn start(limit: Duration, runtime: Duration) -> Result<Self, ProbeFailure> {
        Self::at(limit, runtime, Instant::now())
    }

    pub(super) fn at(
        limit: Duration,
        runtime: Duration,
        admitted: Instant,
    ) -> Result<Self, ProbeFailure> {
        let duration = limit.min(runtime);
        let Some(hard) = admitted.checked_add(duration) else {
            return Err(ProbeFailure::new("probe_deadline_unavailable"));
        };
        let reserve = Duration::from_millis(500).min(duration / 4);
        let operation = hard.checked_sub(reserve).unwrap_or(admitted);
        Ok(Self {
            #[cfg(test)]
            admitted,
            operation,
            hard,
            #[cfg(test)]
            duration,
        })
    }

    pub(super) fn remaining_operation(&self) -> Duration {
        self.remaining_operation_at(Instant::now())
    }

    pub(super) fn remaining_hard(&self) -> Duration {
        self.remaining_hard_at(Instant::now())
    }

    pub(super) fn remaining_operation_at(&self, now: Instant) -> Duration {
        self.operation.saturating_duration_since(now)
    }

    pub(super) fn remaining_hard_at(&self, now: Instant) -> Duration {
        self.hard.saturating_duration_since(now)
    }

    pub(super) fn expired_operation(&self) -> bool {
        self.expired_operation_at(Instant::now())
    }

    pub(super) fn expired_operation_at(&self, now: Instant) -> bool {
        now >= self.operation
    }

    #[cfg(test)]
    pub(super) fn receipt(&self) -> DeadlineReceipt {
        DeadlineReceipt {
            admitted: self.admitted,
            operation: self.operation,
            hard: self.hard,
            duration: self.duration,
        }
    }

    pub(super) fn record_lateness(&self, failure: &mut ProbeFailure) {
        let now = Instant::now();
        if now > self.hard {
            failure.late_by = Some(now.duration_since(self.hard));
        }
    }
}

#[cfg(any(unix, test))]
pub(super) fn cancellation_target(
    ownership: LeaderOwnership,
    child_id: u32,
) -> Result<i32, String> {
    match ownership {
        LeaderOwnership::Retained(id) if id == child_id && id > 1 && id <= i32::MAX as u32 => {
            Ok(id as i32)
        }
        _ => Err("probe child is reaped or unowned; destructive group cleanup withheld".into()),
    }
}

#[cfg(any(windows, test))]
#[derive(Default)]
pub(super) struct WindowsLaunchState {
    job_ready: bool,
    capture_ready: bool,
    process_captured: bool,
    assigned: bool,
    resumed: bool,
}

#[cfg(any(windows, test))]
impl WindowsLaunchState {
    pub(super) fn job_ready(&mut self) {
        self.job_ready = true;
    }

    pub(super) fn capture_ready(&mut self) {
        self.capture_ready = true;
    }

    pub(super) fn process_captured(&mut self) -> Result<(), &'static str> {
        if !self.job_ready || !self.capture_ready {
            return Err("containment and capture must precede process creation");
        }
        self.process_captured = true;
        Ok(())
    }

    pub(super) fn assigned(&mut self) -> Result<(), &'static str> {
        if !self.process_captured {
            return Err("captured process handle required before Job assignment");
        }
        self.assigned = true;
        Ok(())
    }

    pub(super) fn resume(&mut self) -> Result<(), &'static str> {
        if !self.job_ready || !self.capture_ready || !self.process_captured || !self.assigned {
            return Err("contained captured process required before resume");
        }
        if self.resumed {
            return Err("captured process cannot resume twice");
        }
        self.resumed = true;
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn resumed(&self) -> bool {
        self.resumed
    }
}
