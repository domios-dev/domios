use std::io;
use std::os::fd::{IntoRawFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use super::cancellation::{
    unix::{Events, ParentEvent},
    Cancellation,
};
use super::capture::{self, CaptureData};
use super::environment::Environment;
use super::owner::{ProbeResourceOwner, Reservation, RetainedResources, Settlement};
use super::process::{Deadlines, LeaderOwnership, ProbeFailure, ResourceKind};
use super::Output;
use wait_timeout::ChildExt;

const DRAIN_BYTES: usize = 16 * 1024;

#[derive(Default)]
struct Stream {
    fd: Option<RawFd>,
    data: CaptureData,
    registered: bool,
    settled: bool,
    read_failed: bool,
    close_unknown: bool,
    _uncertain_fd: Option<RawFd>,
}

pub(super) fn run(
    command: &str,
    cwd: &Path,
    deadlines: Deadlines,
    reservation: Reservation,
    owner: &Arc<ProbeResourceOwner>,
    cancellation: &Cancellation,
    cancellation_required: bool,
    environment: &Environment,
) -> Result<Output, ProbeFailure> {
    let mut process = UnixRun::new(deadlines, reservation, owner);
    if cancellation_required {
        match Events::new(cancellation) {
            Ok(events) => {
                process.events = Some(events);
                process.events_registered = process.register_resource(ResourceKind::Capture);
                if !process.events_registered {
                    process.set_failure("probe_cancel_registration_failed");
                    process.permanent_unconfirmed = true;
                    return process.finish_failure();
                }
            }
            Err(error) => {
                process.set_failure(
                    if error.starts_with("probe_cancel_parent_wait_unsupported_kernel") {
                        "probe_cancel_parent_wait_unsupported_kernel"
                    } else if error.starts_with("probe_cancel_parent_wait_unsupported_platform") {
                        "probe_cancel_parent_wait_unsupported_platform"
                    } else {
                        "probe_cancel_registration_failed"
                    },
                );
                process.add_cleanup_error(error);
                return process.finish_failure();
            }
        }
    }
    if cancellation.load(std::sync::atomic::Ordering::SeqCst) {
        process.set_failure("probe_cancelled");
        return process.finish_failure();
    }
    if owner.is_closing() || process.deadlines.expired_operation() {
        process.failure = Some(ProbeFailure::new(if owner.is_closing() {
            "probe_owner_closing"
        } else {
            "probe_deadline_before_spawn"
        }));
        return process.finish_failure();
    }
    let mut child_command = Command::new("/bin/sh");
    child_command
        .args(["-c", command])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    child_command.envs(
        environment
            .variables
            .iter()
            .map(|(key, value)| (key, value)),
    );
    for key in &environment.removed {
        child_command.env_remove(key);
    }
    if cancellation.load(std::sync::atomic::Ordering::SeqCst) {
        process.set_failure("probe_cancelled");
        return process.finish_failure();
    }
    match child_command.spawn() {
        Ok(child) => process.adopt_child(child),
        Err(error) => {
            process.failure = Some(ProbeFailure::new("probe_spawn_failed"));
            process.add_cleanup_error(error.to_string());
            return process.finish_failure();
        }
    }
    process.adopt_streams();
    if let (Some(events), Some(child)) = (&mut process.events, &mut process.child) {
        if let Err(error) = events.observe_parent(child) {
            match child.try_wait() {
                Ok(Some(status)) => process.parent_finished(status),
                Ok(None) => {
                    process.set_failure("probe_parent_event_registration_failed");
                    process.add_cleanup_error(error);
                }
                Err(wait_error) => {
                    process.ownership = LeaderOwnership::Lost;
                    process.permanent_unconfirmed = true;
                    process.set_failure("probe_wait_failed");
                    process.add_cleanup_error(wait_error.to_string());
                }
            }
        }
    }
    process.set_nonblocking();
    process.drive()
}

struct UnixRun {
    deadlines: Deadlines,
    owner: Weak<ProbeResourceOwner>,
    reservation: Option<Reservation>,
    child: Option<Child>,
    child_id: u32,
    child_registered: bool,
    ownership: LeaderOwnership,
    group_registered: bool,
    group_retired: bool,
    group_unconfirmed: bool,
    stdout: Stream,
    stderr: Stream,
    status: Option<ExitStatus>,
    cancel_attempted: bool,
    failure: Option<ProbeFailure>,
    permanent_unconfirmed: bool,
    events: Option<Events>,
    events_registered: bool,
}

impl UnixRun {
    fn new(
        deadlines: Deadlines,
        reservation: Reservation,
        owner: &Arc<ProbeResourceOwner>,
    ) -> Self {
        Self {
            deadlines,
            owner: Arc::downgrade(owner),
            reservation: Some(reservation),
            child: None,
            child_id: 0,
            child_registered: false,
            ownership: LeaderOwnership::Unspawned,
            group_registered: false,
            group_retired: false,
            group_unconfirmed: false,
            stdout: Stream::default(),
            stderr: Stream::default(),
            status: None,
            cancel_attempted: false,
            failure: None,
            permanent_unconfirmed: false,
            events: None,
            events_registered: false,
        }
    }

    fn adopt_child(&mut self, child: Child) {
        self.child_id = child.id();
        self.child = Some(child);
        self.ownership = if self.child_id > 1 && self.child_id <= i32::MAX as u32 {
            LeaderOwnership::Retained(self.child_id)
        } else {
            LeaderOwnership::Lost
        };
        self.child_registered = self.register_resource(ResourceKind::Child);
        self.group_registered = self.register_resource(ResourceKind::ProcessGroup);
        if !self.child_registered || !self.group_registered {
            self.set_failure("probe_resource_registration_failed");
            self.permanent_unconfirmed = true;
            self.mark_unconfirmed(ResourceKind::Child);
            self.mark_unconfirmed(ResourceKind::ProcessGroup);
        }
        if self.ownership == LeaderOwnership::Lost {
            self.set_failure("probe_process_ownership_failed");
            self.permanent_unconfirmed = true;
            self.mark_unconfirmed(ResourceKind::ProcessGroup);
        }
    }

    fn adopt_streams(&mut self) {
        let stdout = self.child.as_mut().and_then(|child| child.stdout.take());
        match stdout {
            Some(stream) => self.adopt_stream(stream.into_raw_fd(), true),
            None => self.set_failure("probe_capture_failed"),
        }
        let stderr = self.child.as_mut().and_then(|child| child.stderr.take());
        match stderr {
            Some(stream) => self.adopt_stream(stream.into_raw_fd(), false),
            None => self.set_failure("probe_capture_failed"),
        }
    }

    fn adopt_stream(&mut self, fd: RawFd, stdout: bool) {
        if stdout {
            self.stdout.fd = Some(fd);
        } else {
            self.stderr.fd = Some(fd);
        }
        let registered = self.register_resource(ResourceKind::Capture);
        if stdout {
            self.stdout.registered = registered;
        } else {
            self.stderr.registered = registered;
        }
        if !registered {
            self.set_failure("probe_capture_failed");
            self.permanent_unconfirmed = true;
            self.mark_unconfirmed(ResourceKind::Capture);
        }
    }

    fn set_nonblocking(&mut self) {
        for stdout in [true, false] {
            let fd = self.stream(stdout).fd;
            let Some(fd) = fd else {
                continue;
            };
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
            if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
            {
                self.set_failure("probe_capture_failed");
                self.close_stream(stdout);
            }
        }
    }

    fn drive(mut self) -> Result<Output, ProbeFailure> {
        while self.has_open_streams() && self.failure.is_none() {
            if self.events.as_ref().is_some_and(Events::cancelled)
                || self.owner_is_closing()
                || self.deadlines.expired_operation()
            {
                self.set_failure(if self.events.as_ref().is_some_and(Events::cancelled) {
                    "probe_cancelled"
                } else if self.owner_is_closing() {
                    "probe_owner_closing"
                } else {
                    "probe_timeout"
                });
                break;
            }
            match self.wait_streams(Some(self.deadlines.remaining_operation())) {
                Ok(true) => {}
                Ok(false) => {
                    self.set_failure("probe_timeout");
                    break;
                }
                Err(error) => {
                    self.set_failure("probe_capture_failed");
                    self.add_cleanup_error(error.to_string());
                    break;
                }
            }
        }
        if self.failure.is_none() && !self.has_open_streams() && self.all_streams_eof() {
            match self.wait_parent(self.deadlines.remaining_operation()) {
                Ok(true) => self.observe_final_group(),
                Ok(false) => self.set_failure("probe_timeout"),
                Err(error) => {
                    self.set_failure("probe_wait_failed");
                    self.add_cleanup_error(error);
                }
            }
        }
        if let Some(mut failure) = self.failure.take() {
            self.cleanup(&mut failure);
            self.deadlines.record_lateness(&mut failure);
            self.finish_failure_with(failure)
        } else if self.deadlines.expired_operation() {
            let mut failure = ProbeFailure::new("probe_timeout");
            self.cleanup(&mut failure);
            self.deadlines.record_lateness(&mut failure);
            self.finish_failure_with(failure)
        } else if self.status.is_some() && self.group_retired && self.all_streams_eof() {
            let status = self.status.unwrap();
            if Instant::now() >= self.deadlines.operation {
                let mut failure = ProbeFailure::new("probe_timeout");
                self.cleanup(&mut failure);
                self.deadlines.record_lateness(&mut failure);
                self.finish_failure_with(failure)
            } else if self.close_event_resources().is_err() {
                self.permanent_unconfirmed = true;
                self.set_failure("probe_close_failed");
                self.mark_unconfirmed(ResourceKind::Capture);
                let failure = self.failure.take().unwrap();
                self.finish_failure_with(failure)
            } else if Instant::now() >= self.deadlines.operation {
                let mut failure = ProbeFailure::new("probe_timeout");
                self.deadlines.record_lateness(&mut failure);
                self.finish_failure_with(failure)
            } else if Instant::now() > self.deadlines.hard {
                let mut failure = ProbeFailure::new("probe_cleanup_deadline");
                self.deadlines.record_lateness(&mut failure);
                self.finish_failure_with(failure)
            } else {
                let output = match capture::assemble_confirmed(
                    status,
                    std::mem::take(&mut self.stdout.data),
                    std::mem::take(&mut self.stderr.data),
                    capture::CompletionFacts {
                        parent_complete: true,
                        boundary_complete: true,
                        stdout_eof: true,
                        stderr_eof: true,
                        error: None,
                    },
                ) {
                    Ok(output) => output,
                    Err(failure) => return self.finish_failure_with(failure),
                };
                if let Err(error) = self.release_reservation() {
                    let mut failure = ProbeFailure::new("probe_owner_release_failed");
                    failure.cleanup_error(error);
                    failure.unconfirmed(ResourceKind::Capture);
                    return Err(failure.with_output(&output));
                }
                if Instant::now() > self.deadlines.hard {
                    let mut failure = ProbeFailure::new("probe_cleanup_deadline");
                    self.deadlines.record_lateness(&mut failure);
                    return Err(failure.with_output(&output));
                }
                Ok(output)
            }
        } else {
            let mut failure = ProbeFailure::new("probe_completion_unconfirmed");
            self.mark_unconfirmed(ResourceKind::ProcessGroup);
            failure.unconfirmed(ResourceKind::ProcessGroup);
            self.cleanup(&mut failure);
            self.deadlines.record_lateness(&mut failure);
            self.finish_failure_with(failure)
        }
    }

    fn wait_streams(&mut self, timeout: Option<Duration>) -> Result<bool, io::Error> {
        let mut fds = [
            libc::pollfd {
                fd: if self.cancel_attempted {
                    -1
                } else {
                    self.events.as_ref().map_or(-1, Events::cancel_fd)
                },
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: self.stdout.fd.unwrap_or(-1),
                events: libc::POLLIN | libc::POLLHUP | libc::POLLERR,
                revents: 0,
            },
            libc::pollfd {
                fd: self.stderr.fd.unwrap_or(-1),
                events: libc::POLLIN | libc::POLLHUP | libc::POLLERR,
                revents: 0,
            },
        ];
        #[cfg(test)]
        if let Some(events) = &self.events {
            events.waiting(super::cancellation::WaitPhase::Capture);
        }
        let result = unsafe {
            libc::poll(
                fds.as_mut_ptr(),
                fds.len() as _,
                timeout.map(timeout_ms).unwrap_or(-1),
            )
        };
        if result < 0 {
            let error = io::Error::last_os_error();
            return if error.kind() == io::ErrorKind::Interrupted {
                Ok(true)
            } else {
                Err(error)
            };
        }
        if result == 0 {
            return Ok(false);
        }
        if fds[0].revents != 0 {
            self.set_failure("probe_cancelled");
            return Ok(true);
        }
        let stdout_ready = fds[1].revents != 0;
        let stderr_ready = fds[2].revents != 0;
        let mut changed = false;
        if stdout_ready {
            changed |= self.drain_stream(true)?;
        }
        if stderr_ready {
            changed |= self.drain_stream(false)?;
        }
        if changed {
            self.observe_group_if_complete();
        }
        Ok(true)
    }

    fn drain_stream(&mut self, stdout: bool) -> Result<bool, io::Error> {
        let mut drained = 0;
        let mut changed = false;
        while drained < DRAIN_BYTES {
            let fd = self.stream(stdout).fd;
            let Some(fd) = fd else {
                break;
            };
            let mut buffer = [0u8; 4096];
            let read = unsafe {
                libc::read(
                    fd,
                    buffer.as_mut_ptr().cast(),
                    buffer.len().min(DRAIN_BYTES - drained),
                )
            };
            if read > 0 {
                self.stream_mut(stdout)
                    .data
                    .append(&buffer[..read as usize]);
                drained += read as usize;
                changed = true;
                continue;
            }
            if read == 0 {
                self.stream_mut(stdout).data.mark_eof();
                changed = true;
                self.close_stream(stdout);
                break;
            }
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            if error.kind() == io::ErrorKind::WouldBlock {
                break;
            }
            self.stream_mut(stdout).read_failed = true;
            self.set_failure("probe_capture_failed");
            self.add_cleanup_error(error.to_string());
            self.close_stream(stdout);
            break;
        }
        Ok(changed)
    }

    fn wait_parent(&mut self, timeout: Duration) -> Result<bool, String> {
        let Some(child) = self.child.as_mut() else {
            return Ok(self.status.is_some());
        };
        let result = if let Some(events) = &self.events {
            match events.wait_parent(timeout, !self.cancel_attempted)? {
                ParentEvent::Cancelled => {
                    self.set_failure("probe_cancelled");
                    return Ok(false);
                }
                ParentEvent::Deadline => return Ok(false),
                ParentEvent::Complete => child.wait().map(Some),
            }
        } else {
            child.wait_timeout(timeout)
        };
        match result {
            Ok(Some(status)) => {
                self.parent_finished(status);
                Ok(true)
            }
            Ok(None) => Ok(false),
            Err(error) => {
                self.ownership = LeaderOwnership::Lost;
                self.permanent_unconfirmed = true;
                Err(error.to_string())
            }
        }
    }

    fn parent_finished(&mut self, status: ExitStatus) {
        self.status = Some(status);
        self.ownership = LeaderOwnership::Retired(self.child_id);
        self.child = None;
        let registered = std::mem::replace(&mut self.child_registered, false);
        self.settle_resource(ResourceKind::Child, registered);
    }

    fn observe_final_group(&mut self) {
        if self.group_retired {
            return;
        }
        if self.ownership == LeaderOwnership::Lost
            || self.child_id <= 1
            || self.child_id > i32::MAX as u32
        {
            self.group_unconfirmed = true;
            self.set_failure("probe_group_completion_unconfirmed");
            self.mark_unconfirmed(ResourceKind::ProcessGroup);
            return;
        }
        if self.ownership != LeaderOwnership::Retired(self.child_id) || self.has_open_streams() {
            self.group_unconfirmed = true;
            self.mark_unconfirmed(ResourceKind::ProcessGroup);
            return;
        }
        let observation = super::group_retirement::observe_retired(self.ownership, self.child_id);
        if matches!(observation, Ok(true)) {
            self.group_retired = true;
            self.group_unconfirmed = false;
            if let Some(reservation) = self.reservation.as_mut() {
                reservation.release_process_admission();
            }
            let registered = std::mem::replace(&mut self.group_registered, false);
            self.settle_resource(ResourceKind::ProcessGroup, registered);
        } else {
            self.group_unconfirmed = true;
            self.set_failure("probe_group_completion_unconfirmed");
            self.mark_unconfirmed(ResourceKind::ProcessGroup);
            if let Err(error) = observation {
                self.add_cleanup_error(error);
            }
        }
    }

    fn observe_group_if_complete(&mut self) {
        if self.status.is_some() && self.all_streams_eof() && !self.group_retired {
            self.observe_final_group();
        }
    }

    fn cleanup(&mut self, failure: &mut ProbeFailure) {
        let retirement_deadline = self
            .deadlines
            .hard
            .max(Instant::now() + super::PROBE_RUNTIME);
        if self.ownership == LeaderOwnership::Retained(self.child_id) {
            self.request_cancel(failure);
            match super::group_retirement::retire(
                self.ownership,
                self.child_id,
                retirement_deadline,
            ) {
                Ok(()) => {
                    self.group_retired = true;
                    self.group_unconfirmed = false;
                    let registered = std::mem::replace(&mut self.group_registered, false);
                    self.settle_resource(ResourceKind::ProcessGroup, registered);
                }
                Err(error) => {
                    failure.cleanup_error(error);
                    self.group_unconfirmed = true;
                }
            }
        } else if matches!(
            self.ownership,
            LeaderOwnership::Retired(_) | LeaderOwnership::Lost
        ) && !self.group_retired
        {
            self.group_unconfirmed = true;
            self.mark_unconfirmed(ResourceKind::ProcessGroup);
            failure.unconfirmed(ResourceKind::ProcessGroup);
        }
        let capture_deadline = if self.group_retired && Instant::now() > self.deadlines.hard {
            retirement_deadline
        } else {
            self.deadlines.hard
        };
        while self.has_open_streams() && Instant::now() < capture_deadline {
            match self.wait_streams(Some(
                capture_deadline.saturating_duration_since(Instant::now()),
            )) {
                Ok(true) => {}
                Ok(false) => break,
                Err(error) => {
                    failure.cleanup_error(error.to_string());
                    self.close_all_streams(failure);
                    break;
                }
            }
            if self.owner_is_closing() && self.ownership == LeaderOwnership::Retained(self.child_id)
            {
                self.request_cancel(failure);
            }
        }
        if !self.has_open_streams()
            && self.group_retired
            && self.ownership == LeaderOwnership::Retained(self.child_id)
        {
            match self.wait_parent(retirement_deadline.saturating_duration_since(Instant::now())) {
                Ok(true) => {}
                Ok(false) => failure.unconfirmed(ResourceKind::Child),
                Err(error) => {
                    failure.cleanup_error(error);
                    failure.unconfirmed(ResourceKind::Child);
                }
            }
        }
        if self.status.is_some() && !self.has_open_streams() && !self.group_retired {
            self.observe_final_group();
        }
        if self.has_open_streams() {
            failure.unconfirmed(ResourceKind::Capture);
        }
        if self.child.is_some() {
            failure.unconfirmed(ResourceKind::Child);
        }
        if self.group_unconfirmed || self.permanent_unconfirmed {
            failure.unconfirmed(ResourceKind::ProcessGroup);
        }
        if let Some(cleanup_failure) = self.failure.take() {
            failure.record_cleanup_errors(cleanup_failure.cleanup_errors);
            for resource in cleanup_failure.unconfirmed {
                failure.unconfirmed(resource);
            }
        }
    }

    fn request_cancel(&mut self, failure: &mut ProbeFailure) {
        if self.cancel_attempted {
            return;
        }
        self.cancel_attempted = true;
        if let Err(error) =
            super::group_retirement::signal_owned_group(self.ownership, self.child_id)
        {
            failure.cleanup_error(error);
        }
    }

    fn close_all_streams(&mut self, failure: &mut ProbeFailure) {
        self.close_stream(true);
        self.close_stream(false);
        if self.stdout.close_unknown || self.stderr.close_unknown {
            failure.cleanup_error("probe capture close completion unconfirmed");
            failure.unconfirmed(ResourceKind::Capture);
        }
    }

    fn close_stream(&mut self, stdout: bool) {
        let Some(fd) = self.stream_mut(stdout).fd.take() else {
            return;
        };
        if unsafe { libc::close(fd) } == 0 {
            self.stream_mut(stdout).settled = true;
            let registered = std::mem::replace(&mut self.stream_mut(stdout).registered, false);
            self.settle_resource(ResourceKind::Capture, registered);
        } else {
            self.stream_mut(stdout).close_unknown = true;
            self.stream_mut(stdout)._uncertain_fd = Some(fd);
            self.permanent_unconfirmed = true;
            self.mark_unconfirmed(ResourceKind::Capture);
        }
    }

    fn close_event_resources(&mut self) -> Result<(), String> {
        let mut errors: Vec<String> = Vec::new();
        self.close_stream(true);
        self.close_stream(false);
        if let Some(events) = &mut self.events {
            match events.close() {
                Ok(()) => {
                    self.events = None;
                    let registered = std::mem::replace(&mut self.events_registered, false);
                    self.settle_resource(ResourceKind::Capture, registered);
                }
                Err(error) => {
                    self.permanent_unconfirmed = true;
                    errors.push(error);
                }
            }
        }
        if self.stdout.close_unknown || self.stderr.close_unknown {
            errors.push("probe capture close completion unconfirmed".into());
        }
        if self.permanent_unconfirmed || self.group_unconfirmed {
            errors.push("probe resource completion unconfirmed".into());
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    fn finish_failure(mut self) -> Result<Output, ProbeFailure> {
        let mut failure = self
            .failure
            .take()
            .unwrap_or_else(|| ProbeFailure::new("probe_failed"));
        self.cleanup(&mut failure);
        self.deadlines.record_lateness(&mut failure);
        self.finish_failure_with(failure)
    }

    fn finish_failure_with(mut self, mut failure: ProbeFailure) -> Result<Output, ProbeFailure> {
        let deadlines = self.deadlines;
        if let Some(error) = self.events.as_ref().and_then(Events::take_delivery_error) {
            failure.primary = "probe_cancel_wake_failed";
            failure.cleanup_error(error);
        }
        failure.processes_confirmed = self.status.is_some()
            && self.child.is_none()
            && self.group_retired
            && !self.group_unconfirmed;
        failure.exit_code = self.status.and_then(|status| status.code());
        failure.stdout = (self.child_id != 0).then(|| self.stdout.data.metadata());
        failure.stderr = (self.child_id != 0).then(|| self.stderr.data.metadata());
        if !self.has_open_resources() {
            if let Err(error) = self.close_event_resources() {
                failure.cleanup_error(error);
            }
        }
        if self.has_open_resources() {
            failure.unconfirmed(if self.child.is_some() {
                ResourceKind::Child
            } else {
                ResourceKind::Capture
            });
            self.retain(&mut failure);
        } else if self.group_unconfirmed || self.permanent_unconfirmed {
            failure.unconfirmed(ResourceKind::ProcessGroup);
            self.retain(&mut failure);
        } else {
            if let Err(error) = self.release_reservation() {
                failure.cleanup_error(error);
                failure.unconfirmed(ResourceKind::Capture);
            }
        }
        deadlines.record_lateness(&mut failure);
        Err(failure)
    }

    fn retain(mut self, failure: &mut ProbeFailure) {
        let Some(reservation) = self.reservation.take() else {
            failure.unconfirmed(ResourceKind::Capture);
            return;
        };
        let pending = UnixPending::from_run(self);
        reservation.retain(Box::new(pending));
    }

    fn release_reservation(&mut self) -> Result<(), String> {
        if let Some(reservation) = self.reservation.take() {
            if let Err(error) = reservation.release() {
                self.permanent_unconfirmed = true;
                self.record_owner_failure(&error);
                return Err(error);
            }
        }
        Ok(())
    }

    fn register_resource(&mut self, kind: ResourceKind) -> bool {
        match self
            .reservation
            .as_mut()
            .map(Reservation::register_resource)
        {
            Some(Ok(())) => true,
            Some(Err(error)) => {
                self.record_owner_failure(format!("{}: {error}", resource_name(kind)));
                false
            }
            None => false,
        }
    }

    fn settle_resource(&mut self, kind: ResourceKind, registered: bool) {
        if !registered {
            return;
        }
        if let Some(reservation) = self.reservation.as_mut() {
            if let Err(error) = reservation.resource_settled() {
                self.permanent_unconfirmed = true;
                self.record_owner_failure(format!("{}: {error}", resource_name(kind)));
            }
        }
    }

    fn set_failure(&mut self, primary: &'static str) {
        if self.failure.is_none() {
            self.failure = Some(ProbeFailure::new(primary));
        }
    }

    fn add_cleanup_error(&mut self, error: String) {
        if let Some(failure) = self.failure.as_mut() {
            failure.cleanup_error(error);
        }
    }

    fn mark_unconfirmed(&mut self, resource: ResourceKind) {
        if let Some(failure) = self.failure.as_mut() {
            failure.unconfirmed(resource);
        }
    }

    fn has_open_streams(&self) -> bool {
        self.stdout.fd.is_some() || self.stderr.fd.is_some()
    }

    fn has_open_resources(&self) -> bool {
        self.has_open_streams() || self.child.is_some()
    }

    fn all_streams_eof(&self) -> bool {
        self.stdout.data.eof && self.stderr.data.eof
    }

    fn stream(&self, stdout: bool) -> &Stream {
        if stdout {
            &self.stdout
        } else {
            &self.stderr
        }
    }

    fn stream_mut(&mut self, stdout: bool) -> &mut Stream {
        if stdout {
            &mut self.stdout
        } else {
            &mut self.stderr
        }
    }

    fn owner_is_closing(&self) -> bool {
        self.owner
            .upgrade()
            .map_or(true, |owner| owner.is_closing())
    }

    fn record_owner_failure(&self, diagnostic: impl AsRef<str>) {
        if let Some(owner) = self.owner.upgrade() {
            owner.record_failure(diagnostic);
        }
    }
}

struct UnixPending {
    run: UnixRun,
}

impl UnixPending {
    fn from_run(mut run: UnixRun) -> Self {
        run.reservation = None;
        Self { run }
    }

    fn wait_events(&mut self) {
        if self.run.ownership == LeaderOwnership::Lost {
            self.run.permanent_unconfirmed = true;
            self.run.group_unconfirmed = true;
            return;
        }
        if self.run.ownership == LeaderOwnership::Retained(self.run.child_id)
            && !self.run.group_retired
        {
            match super::group_retirement::retire(
                self.run.ownership,
                self.run.child_id,
                Instant::now() + super::PROBE_RUNTIME,
            ) {
                Ok(()) => {
                    self.run.group_retired = true;
                    self.run.group_unconfirmed = false;
                }
                Err(error) => {
                    self.run
                        .failure
                        .get_or_insert_with(|| ProbeFailure::new("probe_cleanup"))
                        .cleanup_error(error);
                    return;
                }
            }
        }
        while self.run.has_open_streams() {
            if let Err(error) = self.run.wait_streams(None) {
                self.run
                    .failure
                    .get_or_insert_with(|| ProbeFailure::new("probe_capture_failed"));
                if let Some(failure) = self.run.failure.as_mut() {
                    failure.cleanup_error(error.to_string());
                }
                if self.run.stdout.fd.is_some() {
                    self.run.close_stream(true);
                }
                if self.run.stderr.fd.is_some() {
                    self.run.close_stream(false);
                }
                break;
            }
        }
        if let Some(result) = self.run.child.as_mut().map(Child::wait) {
            match result {
                Ok(status) => {
                    self.run.status = Some(status);
                    self.run.ownership = LeaderOwnership::Retired(self.run.child_id);
                    self.run.child = None;
                    self.run.child_registered = false;
                }
                Err(error) => {
                    self.run.ownership = LeaderOwnership::Lost;
                    self.run.permanent_unconfirmed = true;
                    self.run.group_unconfirmed = true;
                    self.run
                        .failure
                        .get_or_insert_with(|| ProbeFailure::new("probe_wait_failed"));
                    if let Some(failure) = self.run.failure.as_mut() {
                        failure.cleanup_error(error.to_string());
                    }
                }
            }
        }
        if self.run.status.is_some() && !self.run.has_open_streams() {
            self.run.observe_final_group();
        } else if self.run.has_open_streams() {
            self.run.group_unconfirmed = true;
        }
        self.run.close_stream(true);
        self.run.close_stream(false);
        if let Err(error) = self.run.close_event_resources() {
            self.run.permanent_unconfirmed = true;
            self.run
                .failure
                .get_or_insert_with(|| ProbeFailure::new("probe_close_failed"))
                .cleanup_error(error);
        }
    }
}

impl RetainedResources for UnixPending {
    fn processes_settled(&mut self) -> bool {
        if self.run.group_retired {
            if let Some(child) = self.run.child.as_mut() {
                if let Ok(Some(status)) = child.try_wait() {
                    self.run.parent_finished(status);
                }
            }
        }
        if self.run.child_id == 0 {
            return self.run.child.is_none();
        }
        if self.run.child_id <= 1
            || self.run.child_id > i32::MAX as u32
            || self.run.status.is_none()
            || self.run.ownership != LeaderOwnership::Retired(self.run.child_id)
        {
            return false;
        }
        if !self.run.group_retired {
            self.run.group_retired =
                super::group_retirement::observe_retired(self.run.ownership, self.run.child_id)
                    .unwrap_or(false);
        }
        self.run.group_retired
    }

    fn try_settle(&mut self) -> bool {
        self.run.child.is_none()
            && (self.run.child_id == 0 || (self.run.status.is_some() && self.run.group_retired))
            && !self.run.has_open_streams()
            && !self.run.stdout.close_unknown
            && !self.run.stderr.close_unknown
            && self.run.events.is_none()
    }

    fn request_cancel(&mut self) -> Vec<String> {
        let mut failure = ProbeFailure::new("probe_cleanup");
        self.run.request_cancel(&mut failure);
        failure.cleanup_errors
    }

    fn wait_for_settlement(&mut self) -> Settlement {
        self.wait_events();
        let remains_unconfirmed = !self.try_settle();
        let failure = self.run.failure.take();
        Settlement {
            primary: failure.as_ref().map(|failure| failure.primary.to_string()),
            errors: failure
                .map(|failure| failure.cleanup_errors)
                .unwrap_or_default(),
            remains_unconfirmed,
        }
    }
}

fn timeout_ms(timeout: Duration) -> i32 {
    timeout
        .as_millis()
        .saturating_add(u128::from(timeout.subsec_nanos() % 1_000_000 != 0))
        .min(i32::MAX as u128) as i32
}

fn resource_name(kind: ResourceKind) -> &'static str {
    match kind {
        ResourceKind::Child => "child",
        ResourceKind::ProcessGroup => "process-group",
        ResourceKind::Capture => "capture",
        ResourceKind::Job => "job",
        ResourceKind::OverlappedIo => "overlapped-io",
    }
}
