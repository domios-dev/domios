use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::ExitStatusExt;
use std::path::Path;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{
    GetLastError, SetHandleInformation, ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_NOT_FOUND,
    ERROR_OPERATION_ABORTED, ERROR_PIPE_CONNECTED, HANDLE, HANDLE_FLAG_INHERIT,
    INVALID_HANDLE_VALUE, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_FIRST_PIPE_INSTANCE,
    FILE_FLAG_OVERLAPPED, OPEN_EXISTING, PIPE_ACCESS_INBOUND,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectAssociateCompletionPortInformation,
    JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
    JOBOBJECT_ASSOCIATE_COMPLETION_PORT, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::Threading::{
    CreateProcessW, DeleteProcThreadAttributeList, GetExitCodeProcess,
    InitializeProcThreadAttributeList, TerminateProcess, UpdateProcThreadAttribute,
    WaitForSingleObject, CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
    EXTENDED_STARTUPINFO_PRESENT, PROC_THREAD_ATTRIBUTE_HANDLE_LIST, STARTF_USESTDHANDLES,
    STARTUPINFOEXW,
};
use windows_sys::Win32::System::IO::{
    CancelIoEx, CreateIoCompletionPort, GetQueuedCompletionStatus, OVERLAPPED,
};

use super::cancellation::{
    windows::{Events, CANCEL_KEY},
    Cancellation,
};
use super::capture::{self, OwnedHandle, WindowsStream as Stream};
use super::capture::{last_error, resource_name, timeout_ms};
use super::environment::Environment;
use super::owner::{mark_close_uncertain, ProbeResourceOwner, Reservation, WindowsPending};
use super::process::{Deadlines, ProbeFailure, ResourceKind, WindowsLaunchState};
use super::Output;

const READ_BYTES: usize = 4096;
const JOB_KEY: usize = 3;
const STDOUT_KEY: usize = 1;
const STDERR_KEY: usize = 2;
const JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO: u32 = 4;
const JOB_OBJECT_MSG_EXIT_PROCESS: u32 = 7;

mod capture_prep;
mod completion;

static PIPE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
pub mod ordering_tests;

pub fn run(
    command: &str,
    cwd: &Path,
    deadlines: Deadlines,
    reservation: Reservation,
    owner: &Arc<ProbeResourceOwner>,
    cancellation: &Cancellation,
    environment: &Environment,
) -> Result<Output, ProbeFailure> {
    run_with_assignment(
        command,
        cwd,
        deadlines,
        reservation,
        owner,
        cancellation,
        environment,
        |job, process| {
            if unsafe { AssignProcessToJobObject(job, process) } == 0 {
                Err(last_error())
            } else {
                Ok(())
            }
        },
    )
}

pub fn run_with_assignment(
    command: &str,
    cwd: &Path,
    deadlines: Deadlines,
    reservation: Reservation,
    owner: &Arc<ProbeResourceOwner>,
    cancellation: &Cancellation,
    environment: &Environment,
    assign: impl FnOnce(HANDLE, HANDLE) -> Result<(), String>,
) -> Result<Output, ProbeFailure> {
    let mut run = WindowsRun::new(deadlines, reservation, owner);
    if let Err(error) = run.prepare_cancellation(cancellation) {
        run.fail("probe_cancel_registration_failed");
        run.add_error(error);
        return run.finish_failure();
    }
    if run.cancelled() {
        run.fail("probe_cancelled");
        return run.finish_failure();
    }
    run.stdout.buffer_registered = run.register(ResourceKind::OverlappedIo);
    run.stderr.buffer_registered = run.register(ResourceKind::OverlappedIo);
    run.stdout.connect_buffer_registered = run.register(ResourceKind::OverlappedIo);
    run.stderr.connect_buffer_registered = run.register(ResourceKind::OverlappedIo);
    if run.permanent_unconfirmed {
        run.fail("probe_resource_registration_failed");
        return run.finish_failure();
    }
    if run.cancelled() || run.deadlines.expired_operation() || owner.is_closing() {
        run.fail(if run.cancelled() {
            "probe_cancelled"
        } else if owner.is_closing() {
            "probe_owner_closing"
        } else {
            "probe_deadline_before_spawn"
        });
        return run.finish_failure();
    }
    if let Err(error) = run.prepare_containment() {
        run.fail("probe_containment_prepare_failed");
        run.add_error(error);
        return run.finish_failure();
    }
    run.launch_state.job_ready();
    if let Err(error) = run.prepare_capture() {
        run.fail("probe_capture_prepare_failed");
        run.add_error(error);
        return run.finish_failure();
    }
    run.launch_state.capture_ready();
    if run.cancelled() || run.deadlines.expired_operation() || owner.is_closing() {
        run.fail(if run.cancelled() {
            "probe_cancelled"
        } else if owner.is_closing() {
            "probe_owner_closing"
        } else {
            "probe_deadline_before_spawn"
        });
        return run.finish_failure();
    }
    while !run.stream(true).connected || !run.stream(false).connected {
        if run.cancelled() || run.owner_is_closing() || run.deadlines.expired_operation() {
            run.fail(if run.cancelled() {
                "probe_cancelled"
            } else if run.owner_is_closing() {
                "probe_owner_closing"
            } else {
                "probe_timeout"
            });
            return run.finish_failure();
        }
        match run.wait_event(Some(run.deadlines.remaining_operation())) {
            Ok(true) => {}
            Ok(false) => run.fail("probe_timeout"),
            Err(error) => {
                run.fail("probe_capture_connect_failed");
                run.add_error(error);
            }
        }
        if run.failure.is_some() {
            return run.finish_failure();
        }
    }
    if let Err(error) = run.start_read(true).and_then(|_| run.start_read(false)) {
        run.fail("probe_capture_start_failed");
        run.add_error(error);
        return run.finish_failure();
    }
    if let Err(error) = run.spawn_contained(command, cwd, environment, assign) {
        run.fail(if run.deadlines.expired_operation() {
            "probe_timeout"
        } else {
            "probe_containment_failed"
        });
        run.add_error(error);
        return run.finish_failure();
    }
    #[cfg(test)]
    match cancellation.child_ready(run.deadlines) {
        Ok(deadlines) => run.deadlines = deadlines,
        Err(error) => {
            run.fail("probe_fixture_readiness_failed");
            run.add_error(error);
            return run.finish_failure();
        }
    }
    run.drive()
}

pub struct WindowsRun {
    deadlines: Deadlines,
    pub owner: Weak<ProbeResourceOwner>,
    reservation: Option<Reservation>,
    pub job: OwnedHandle,
    pub port: OwnedHandle,
    pub process: OwnedHandle,
    pub primary_thread: OwnedHandle,
    pub stdin: OwnedHandle,
    pub stdout: Stream,
    pub stderr: Stream,
    child_id: u32,
    pub assigned: bool,
    pub parent_exited: bool,
    parent_exit_pending: bool,
    exit_code: Option<u32>,
    pub job_empty: bool,
    pub cancel_attempted: bool,
    pub unknown_completion: bool,
    pub permanent_unconfirmed: bool,
    pub failure: Option<ProbeFailure>,
    pub job_registered: bool,
    pub port_registered: bool,
    process_registered: bool,
    thread_registered: bool,
    stdin_registered: bool,
    launch_state: WindowsLaunchState,
    pub events: Option<Events>,
    pub events_registered: bool,
    #[cfg(test)]
    native_trace_remaining: u8,
    #[cfg(test)]
    native_error_reported: bool,
}

// SAFETY: OS handles transfer across threads and pending OVERLAPPED storage remains boxed.
unsafe impl Send for WindowsRun {}

impl WindowsRun {
    fn new(
        deadlines: Deadlines,
        reservation: Reservation,
        owner: &Arc<ProbeResourceOwner>,
    ) -> Self {
        Self {
            deadlines,
            owner: Arc::downgrade(owner),
            reservation: Some(reservation),
            job: OwnedHandle::empty(),
            port: OwnedHandle::empty(),
            process: OwnedHandle::empty(),
            primary_thread: OwnedHandle::empty(),
            stdin: OwnedHandle::empty(),
            stdout: Stream::new(),
            stderr: Stream::new(),
            child_id: 0,
            assigned: false,
            parent_exited: false,
            parent_exit_pending: false,
            exit_code: None,
            job_empty: true,
            cancel_attempted: false,
            unknown_completion: false,
            permanent_unconfirmed: false,
            failure: None,
            job_registered: false,
            port_registered: false,
            process_registered: false,
            thread_registered: false,
            stdin_registered: false,
            launch_state: WindowsLaunchState::default(),
            events: None,
            events_registered: false,
            #[cfg(test)]
            native_trace_remaining: 32,
            #[cfg(test)]
            native_error_reported: false,
        }
    }

    fn prepare_containment(&mut self) -> Result<(), String> {
        let job = unsafe { CreateJobObjectW(null(), null()) };
        self.job = OwnedHandle::new(job).ok_or_else(last_error)?;
        self.job_registered = self.register(ResourceKind::Job);
        if !self.job_registered {
            return Err("probe Job ownership registration failed".into());
        }
        let port = unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, null_mut(), 0, 1) };
        self.port = OwnedHandle::new(port).ok_or_else(last_error)?;
        if let Some(events) = &self.events {
            events.attach_port(port);
        }
        self.port_registered = self.register(ResourceKind::OverlappedIo);
        if !self.port_registered {
            return Err("probe completion-port registration failed".into());
        }
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                self.job.raw().unwrap(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                std::mem::size_of_val(&limits) as u32,
            )
        } == 0
        {
            return Err(last_error());
        }
        let association = JOBOBJECT_ASSOCIATE_COMPLETION_PORT {
            CompletionKey: JOB_KEY as *mut c_void,
            CompletionPort: self.port.raw().unwrap(),
        };
        if unsafe {
            SetInformationJobObject(
                self.job.raw().unwrap(),
                JobObjectAssociateCompletionPortInformation,
                (&association as *const JOBOBJECT_ASSOCIATE_COMPLETION_PORT).cast(),
                std::mem::size_of_val(&association) as u32,
            )
        } == 0
        {
            return Err(last_error());
        }
        Ok(())
    }

    fn spawn_contained(
        &mut self,
        command: &str,
        cwd: &Path,
        environment: &Environment,
        assign: impl FnOnce(HANDLE, HANDLE) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut handles = [
            self.stdin
                .raw()
                .ok_or_else(|| "probe stdin handle missing".to_string())?,
            self.stdout
                .child_writer
                .raw()
                .ok_or_else(|| "probe stdout writer missing".to_string())?,
            self.stderr
                .child_writer
                .raw()
                .ok_or_else(|| "probe stderr writer missing".to_string())?,
        ];
        let mut size = 0usize;
        unsafe {
            InitializeProcThreadAttributeList(null_mut(), 1, 0, &mut size);
        }
        if size == 0 {
            return Err(last_error());
        }
        let words = (size + std::mem::size_of::<usize>() - 1) / std::mem::size_of::<usize>();
        let mut storage = vec![0usize; words];
        let attributes = storage.as_mut_ptr().cast();
        if unsafe { InitializeProcThreadAttributeList(attributes, 1, 0, &mut size) } == 0 {
            return Err(last_error());
        }
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = handles[0];
        startup.StartupInfo.hStdOutput = handles[1];
        startup.StartupInfo.hStdError = handles[2];
        startup.lpAttributeList = attributes;
        let update_ok = unsafe {
            UpdateProcThreadAttribute(
                attributes,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                handles.as_mut_ptr().cast(),
                std::mem::size_of_val(&handles),
                null_mut(),
                null(),
            )
        } != 0;
        if !update_ok {
            unsafe { DeleteProcThreadAttributeList(attributes) };
            return Err(last_error());
        }
        let mut command_line = capture::windows_command_line(command, self.events.as_ref());
        let mut current_directory = cwd.as_os_str().encode_wide().chain([0]).collect::<Vec<_>>();
        let mut environment_block = (!environment.is_empty()).then(|| environment.windows_block());
        let environment_pointer = environment_block
            .as_mut()
            .map(|block| block.as_mut_ptr().cast())
            .unwrap_or(null_mut());
        let creation_flags = CREATE_SUSPENDED
            | CREATE_NO_WINDOW
            | EXTENDED_STARTUPINFO_PRESENT
            | if environment_block.is_some() {
                CREATE_UNICODE_ENVIRONMENT
            } else {
                0
            };
        let mut information = windows_sys::Win32::System::Threading::PROCESS_INFORMATION::default();
        if self.cancelled() {
            unsafe { DeleteProcThreadAttributeList(attributes) };
            self.fail("probe_cancelled");
            return Err("probe_cancelled".into());
        }
        let created = unsafe {
            CreateProcessW(
                null(),
                command_line.as_mut_ptr(),
                null(),
                null(),
                1,
                creation_flags,
                environment_pointer,
                current_directory.as_mut_ptr(),
                &startup.StartupInfo as *const _,
                &mut information,
            )
        } != 0;
        unsafe { DeleteProcThreadAttributeList(attributes) };
        if !created {
            return Err(last_error());
        }
        self.process = OwnedHandle::new(information.hProcess).ok_or_else(last_error)?;
        self.process_registered = self.register(ResourceKind::Child);
        self.primary_thread = OwnedHandle::new(information.hThread).ok_or_else(last_error)?;
        self.thread_registered = self.register(ResourceKind::Child);
        self.child_id = information.dwProcessId;
        if !self.process_registered || !self.thread_registered {
            return Err("probe process ownership registration failed".into());
        }
        self.launch_state
            .process_captured()
            .map_err(str::to_string)?;
        if self.child_id <= 1 || self.child_id > i32::MAX as u32 {
            return Err("captured probe process id is outside the valid range".into());
        }
        if self.cancelled() {
            self.fail("probe_cancelled");
            return Err("probe_cancelled".into());
        }
        if self.deadlines.expired_operation() || self.owner_is_closing() {
            return Err("probe deadline or owner closure preceded Job assignment".into());
        }
        assign(self.job.raw().unwrap(), self.process.raw().unwrap())?;
        self.assigned = true;
        self.launch_state.assigned().map_err(str::to_string)?;
        self.job_empty = false;
        self.close_child_writers()?;
        if self.cancelled() {
            self.fail("probe_cancelled");
            return Err("probe_cancelled".into());
        }
        if self.deadlines.expired_operation() || self.owner_is_closing() {
            return Err("probe deadline or owner closure preceded process resume".into());
        }
        self.launch_state.resume().map_err(str::to_string)?;
        #[link(name = "ntdll")]
        unsafe extern "system" {
            fn NtResumeProcess(process: HANDLE) -> i32;
        }
        let resume_status = unsafe { NtResumeProcess(self.process.raw().unwrap()) };
        if resume_status < 0 {
            return Err(format!(
                "probe process resume failed: NTSTATUS {resume_status:#x}"
            ));
        }
        self.close_thread_handle()?;
        self.close_stdin_handle()?;
        Ok(())
    }

    fn close_child_writers(&mut self) -> Result<(), String> {
        self.close_writer(true)?;
        self.close_writer(false)?;
        Ok(())
    }

    fn close_writer(&mut self, stdout: bool) -> Result<(), String> {
        self.stream_mut(stdout).child_writer.close()?;
        let registered = std::mem::replace(&mut self.stream_mut(stdout).writer_registered, false);
        self.settle_resource(ResourceKind::Capture, registered);
        Ok(())
    }

    fn close_thread_handle(&mut self) -> Result<(), String> {
        self.primary_thread.close()?;
        let registered = std::mem::replace(&mut self.thread_registered, false);
        self.settle_resource(ResourceKind::Child, registered);
        Ok(())
    }

    fn close_stdin_handle(&mut self) -> Result<(), String> {
        self.stdin.close()?;
        let registered = std::mem::replace(&mut self.stdin_registered, false);
        self.settle_resource(ResourceKind::Capture, registered);
        Ok(())
    }

    fn start_read(&mut self, stdout: bool) -> Result<(), String> {
        if self.deadlines.expired_operation() && self.failure.is_none() {
            self.fail("probe_timeout");
            return Err("probe operation deadline elapsed before capture read".into());
        }
        let active = self.stream(stdout).pending_active;
        if active {
            return Err("probe capture read already pending".into());
        }
        if !self.stream(stdout).io_registered {
            if !self.register(ResourceKind::OverlappedIo) {
                return Err("probe capture completion registration failed".into());
            }
            self.stream_mut(stdout).io_registered = true;
        }
        let pipe = self
            .stream(stdout)
            .reader
            .raw()
            .ok_or_else(|| "probe pipe handle missing".to_string())?;
        let result = {
            let stream = self.stream_mut(stdout);
            stream.pending.overlapped = OVERLAPPED::default();
            unsafe {
                ReadFile(
                    pipe,
                    stream.pending.buffer.as_mut_ptr(),
                    READ_BYTES as u32,
                    null_mut(),
                    &mut stream.pending.overlapped,
                )
            }
        };
        let error = (result == 0).then(|| unsafe { GetLastError() });
        #[cfg(test)]
        self.trace_native("ReadFile", Some(stdout), None, error);
        if let Some(error) = error {
            if error == ERROR_IO_PENDING {
                self.stream_mut(stdout).pending_active = true;
                Ok(())
            } else if error == ERROR_BROKEN_PIPE {
                let registered = {
                    let stream = self.stream_mut(stdout);
                    stream.eof = true;
                    stream.data.mark_eof();
                    stream.pending_active = false;
                    std::mem::replace(&mut stream.io_registered, false)
                };
                self.settle_resource(ResourceKind::OverlappedIo, registered);
                self.close_reader(stdout)
            } else {
                let registered = {
                    let stream = self.stream_mut(stdout);
                    stream.read_failed = true;
                    stream.pending_active = false;
                    std::mem::replace(&mut stream.io_registered, false)
                };
                self.settle_resource(ResourceKind::OverlappedIo, registered);
                Err(completion::native_error("ReadFile", Some(stdout), error))
            }
        } else {
            self.stream_mut(stdout).pending_active = true;
            Ok(())
        }
    }

    fn drive(mut self) -> Result<Output, ProbeFailure> {
        while self.failure.is_none() && !self.runtime_success_ready() {
            if self.cancelled() || self.owner_is_closing() || self.deadlines.expired_operation() {
                self.fail(if self.cancelled() {
                    "probe_cancelled"
                } else if self.owner_is_closing() {
                    "probe_owner_closing"
                } else {
                    "probe_timeout"
                });
                break;
            }
            match self.wait_event(Some(self.deadlines.remaining_operation())) {
                Ok(true) => {}
                Ok(false) => {
                    self.fail("probe_timeout");
                }
                Err(error) => {
                    self.fail("probe_capture_or_completion_failed");
                    self.add_error(error);
                }
            }
        }
        if self.failure.is_none() && self.deadlines.expired_operation() {
            self.fail("probe_timeout");
        }
        if let Some(mut failure) = self.failure.take() {
            self.cleanup_to_hard_end(&mut failure);
            self.deadlines.record_lateness(&mut failure);
            self.finish_failure_with(failure)
        } else if self.runtime_success_ready() {
            let Some(exit_code) = self.exit_code else {
                return self
                    .finish_failure_with(ProbeFailure::new("probe_exit_status_unavailable"));
            };
            let status = std::process::ExitStatus::from_raw(exit_code);
            if let Err(error) = self.close_completed_handles() {
                let error = mark_close_uncertain(&mut self.permanent_unconfirmed, error);
                self.fail("probe_close_failed");
                self.add_error(error);
                let failure = self.failure.take().unwrap();
                self.finish_failure_with(failure)
            } else if self.deadlines.expired_operation() {
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
                    failure.unconfirmed(ResourceKind::OverlappedIo);
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
            self.mark_runtime_unconfirmed(&mut failure);
            self.cleanup_to_hard_end(&mut failure);
            self.deadlines.record_lateness(&mut failure);
            self.finish_failure_with(failure)
        }
    }

    fn observe_job_accounting(&mut self) -> Result<(), String> {
        let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        if unsafe {
            QueryInformationJobObject(
                self.job
                    .raw()
                    .ok_or_else(|| "probe Job handle missing".to_string())?,
                JobObjectBasicAccountingInformation,
                (&mut accounting as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                std::mem::size_of_val(&accounting) as u32,
                null_mut(),
            )
        } == 0
        {
            return Err(last_error());
        }
        if accounting.ActiveProcesses == 0 {
            self.job_empty = true;
            self.release_process_admission();
        }
        Ok(())
    }

    fn observe_job_event(&mut self, message: u32, process_id: u32) -> Result<(), String> {
        if !matches!(message, 1..=12) {
            return Err("probe Job packet did not contain a known notification".into());
        }
        // Confirmed retirement can precede queued Job packets while pipe reads still drain.
        if !(self.parent_exited && self.job_empty) {
            self.observe_job_accounting()?;
        }
        if message == JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO {
            self.job_empty = true;
        }
        if message == JOB_OBJECT_MSG_EXIT_PROCESS && process_id == self.child_id {
            // Job notification can precede process signaling; retain the handle for its own wait.
            self.parent_exit_pending = self.process.is_open();
            self.observe_parent_exit_if_signaled()?;
        } else if self.process.is_open() {
            self.observe_parent_exit_if_signaled()?;
        }
        self.close_completed_runtime_handles()?;
        Ok(())
    }

    fn observe_read_event(
        &mut self,
        stdout: bool,
        overlapped: *mut OVERLAPPED,
        bytes: u32,
        error: Option<u32>,
    ) -> Result<(), String> {
        let connect = &mut *self.stream_mut(stdout).connect_overlapped as *mut OVERLAPPED;
        #[cfg(test)]
        self.trace_native(
            if overlapped == connect {
                "ConnectNamedPipe completion"
            } else {
                "ReadFile completion"
            },
            Some(stdout),
            Some(bytes),
            error,
        );
        if overlapped == connect {
            if !self.stream(stdout).connect_pending {
                self.unknown_completion = true;
                return Err("probe pipe connection completion was not pending".into());
            }
            let registered = {
                let stream = self.stream_mut(stdout);
                stream.connect_pending = false;
                if error.is_none() {
                    stream.connected = true;
                } else {
                    stream.read_failed = true;
                }
                std::mem::replace(&mut stream.connect_registered, false)
            };
            self.settle_resource(ResourceKind::OverlappedIo, registered);
            let buffer_registered = std::mem::replace(
                &mut self.stream_mut(stdout).connect_buffer_registered,
                false,
            );
            self.settle_resource(ResourceKind::OverlappedIo, buffer_registered);
            if let Some(error) = error {
                return Err(completion::native_error(
                    "ConnectNamedPipe completion",
                    Some(stdout),
                    error,
                ));
            }
            return Ok(());
        }
        let expected = &mut self.stream_mut(stdout).pending.overlapped as *mut OVERLAPPED;
        if overlapped != expected || !self.stream(stdout).pending_active {
            self.unknown_completion = true;
            self.permanent_unconfirmed = true;
            return Err("probe completion did not match an active owned read".into());
        }
        let registered = {
            let stream = self.stream_mut(stdout);
            stream.pending_active = false;
            std::mem::replace(&mut stream.io_registered, false)
        };
        self.settle_resource(ResourceKind::OverlappedIo, registered);
        if error == Some(ERROR_BROKEN_PIPE) {
            self.stream_mut(stdout).eof = true;
            self.stream_mut(stdout).data.mark_eof();
            return self.close_reader(stdout);
        }
        if let Some(error) = error {
            self.stream_mut(stdout).read_failed = true;
            if error != ERROR_OPERATION_ABORTED {
                self.fail("probe_capture_read_failed");
                self.add_error(completion::native_error(
                    "ReadFile completion",
                    Some(stdout),
                    error,
                ));
            }
            if self.failure.is_some() {
                self.close_reader(stdout)?;
            }
            return Ok(());
        }
        if bytes == 0 {
            self.stream_mut(stdout).read_failed = true;
            self.fail("probe_capture_empty_completion");
            self.close_reader(stdout)?;
            return Ok(());
        }
        if bytes as usize > READ_BYTES {
            self.unknown_completion = true;
            self.permanent_unconfirmed = true;
            return Err("probe read completion exceeded its owned buffer".into());
        }
        let stream = self.stream_mut(stdout);
        stream.data.append(&stream.pending.buffer[..bytes as usize]);
        let _ = stream;
        if self.failure.is_none() && !self.deadlines.expired_operation() {
            self.start_read(stdout)?;
        } else {
            self.fail("probe_timeout");
            self.close_reader(stdout)?;
        }
        Ok(())
    }

    fn cleanup_to_hard_end(&mut self, failure: &mut ProbeFailure) {
        self.request_cancel(failure);
        if !self.assigned && self.process.is_open() {
            self.wait_captured_process(failure);
        }
        for stdout in [true, false] {
            if self.stream(stdout).child_writer.is_open() {
                if let Err(error) = self.close_writer(stdout) {
                    failure.cleanup_error(error);
                    self.permanent_unconfirmed = true;
                }
            }
        }
        if self.stdin.is_open() {
            if let Err(error) = self.close_stdin_handle() {
                failure.cleanup_error(error);
                self.permanent_unconfirmed = true;
            }
        }
        if !self.assigned && !self.process.is_open() {
            if let Err(error) = self.close_unassigned_resources() {
                failure.cleanup_error(error);
                self.permanent_unconfirmed = true;
            }
        }
        while Instant::now() < self.deadlines.hard && !self.failure_resources_settled() {
            match self.wait_event(Some(self.deadlines.remaining_hard())) {
                Ok(true) => {}
                Ok(false) => break,
                Err(error) => {
                    failure.cleanup_error(error);
                    if self.permanent_unconfirmed || self.unknown_completion {
                        failure.unconfirmed(ResourceKind::OverlappedIo);
                        break;
                    }
                }
            }
            self.settle_cancelled_reads(failure);
            self.settle_idle_buffers();
            if !self.assigned && !self.process.is_open() {
                if self.primary_thread.is_open() {
                    if let Err(error) = self.close_thread_handle() {
                        failure.cleanup_error(error);
                        self.permanent_unconfirmed = true;
                    }
                }
                if let Err(error) = self.close_unassigned_resources() {
                    failure.cleanup_error(error);
                    self.permanent_unconfirmed = true;
                }
            }
        }
        self.settle_cancelled_reads(failure);
        self.settle_idle_buffers();
        if self.assigned && self.job_empty && self.parent_exited {
            if let Err(error) = self.close_completed_runtime_handles() {
                failure.cleanup_error(error);
                self.permanent_unconfirmed = true;
            }
        } else if !self.assigned {
            if self.primary_thread.is_open() && self.parent_exited {
                if let Err(error) = self.close_thread_handle() {
                    failure.cleanup_error(error);
                    self.permanent_unconfirmed = true;
                }
            }
            if !self.process.is_open() {
                self.job_empty = true;
                if let Err(error) = self.close_unassigned_resources() {
                    failure.cleanup_error(error);
                    self.permanent_unconfirmed = true;
                }
            }
        }
        if self.process.is_open() {
            if let Err(error) = self.observe_parent_exit_if_signaled() {
                failure.cleanup_error(error);
            }
        }
        if self.assigned && !self.job_empty && self.job.raw().is_some() {
            if let Err(error) = self.observe_job_accounting() {
                failure.cleanup_error(error);
            }
        }
        if let Err(error) = self.close_completed_runtime_handles() {
            failure.cleanup_error(error);
        }
        if !self.failure_resources_settled() || self.permanent_unconfirmed {
            self.mark_runtime_unconfirmed(failure);
        }
    }

    pub fn request_cancel(&mut self, failure: &mut ProbeFailure) {
        if self.cancel_attempted {
            return;
        }
        self.cancel_attempted = true;
        if !self.parent_exited {
            let result = if self.assigned {
                unsafe { TerminateJobObject(self.job.raw().unwrap_or(null_mut()), 1) }
            } else if let Some(process) = self.process.raw() {
                unsafe { TerminateProcess(process, 1) }
            } else {
                1
            };
            if result == 0 {
                failure.cleanup_error(last_error());
            }
        }
        for stdout in [true, false] {
            let stream = self.stream_mut(stdout);
            if stream.connect_pending {
                let result = unsafe {
                    CancelIoEx(
                        stream.reader.raw().unwrap_or(null_mut()),
                        &*stream.connect_overlapped,
                    )
                };
                if result == 0 {
                    let error = unsafe { GetLastError() };
                    if error != ERROR_NOT_FOUND {
                        failure.cleanup_error(
                            std::io::Error::from_raw_os_error(error as i32).to_string(),
                        );
                    }
                }
            }
            if stream.pending_active {
                let result = unsafe {
                    CancelIoEx(
                        stream.reader.raw().unwrap_or(null_mut()),
                        &stream.pending.overlapped,
                    )
                };
                if result == 0 {
                    let error = unsafe { GetLastError() };
                    if error != ERROR_NOT_FOUND {
                        failure.cleanup_error(
                            std::io::Error::from_raw_os_error(error as i32).to_string(),
                        );
                    }
                }
            }
        }
    }

    fn wait_captured_process(&mut self, failure: &mut ProbeFailure) {
        let Some(process) = self.process.raw() else {
            return;
        };
        if unsafe { WaitForSingleObject(process, 0) } == WAIT_OBJECT_0 {
            if let Err(error) = self.observe_parent_exit() {
                failure.cleanup_error(error);
                self.permanent_unconfirmed = true;
            }
            return;
        }
        let remaining = self.deadlines.remaining_hard();
        if unsafe { WaitForSingleObject(process, timeout_ms(remaining)) } == WAIT_OBJECT_0 {
            if let Err(error) = self.observe_parent_exit() {
                failure.cleanup_error(error);
                self.permanent_unconfirmed = true;
            }
        } else {
            failure.unconfirmed(ResourceKind::Child);
        }
    }

    fn close_unassigned_resources(&mut self) -> Result<(), String> {
        for stdout in [true, false] {
            if self.stream(stdout).connect_pending || self.stream(stdout).pending_active {
                return Ok(());
            }
            if self.stream(stdout).reader.is_open() {
                self.close_reader(stdout)?;
            }
        }
        self.close_child_writers()?;
        self.close_stdin_handle()?;
        self.job.close()?;
        let registered = std::mem::replace(&mut self.job_registered, false);
        self.settle_resource(ResourceKind::Job, registered);
        self.detach_cancel_port();
        self.port.close()?;
        let registered = std::mem::replace(&mut self.port_registered, false);
        self.settle_resource(ResourceKind::OverlappedIo, registered);
        self.close_cancellation()
    }

    fn mark_runtime_unconfirmed(&self, failure: &mut ProbeFailure) {
        if self.process.is_open() {
            failure.unconfirmed(ResourceKind::Child);
        }
        if self.job.is_open() || (self.assigned && !self.job_empty) {
            failure.unconfirmed(ResourceKind::Job);
        }
        if self.port.is_open()
            || self.stdout.pending_active
            || self.stderr.pending_active
            || self.stdout.connect_pending
            || self.stderr.connect_pending
        {
            failure.unconfirmed(ResourceKind::OverlappedIo);
        }
        if self.stdout.reader.is_open()
            || self.stderr.reader.is_open()
            || self.stdout.child_writer.is_open()
            || self.stderr.child_writer.is_open()
        {
            failure.unconfirmed(ResourceKind::Capture);
        }
        if self.permanent_unconfirmed || self.unknown_completion {
            failure.unconfirmed(ResourceKind::OverlappedIo);
        }
    }

    fn finish_failure(mut self) -> Result<Output, ProbeFailure> {
        let mut failure = self
            .failure
            .take()
            .unwrap_or_else(|| ProbeFailure::new("probe_failed"));
        self.cleanup_to_hard_end(&mut failure);
        self.deadlines.record_lateness(&mut failure);
        self.finish_failure_with(failure)
    }

    fn finish_failure_with(mut self, mut failure: ProbeFailure) -> Result<Output, ProbeFailure> {
        let deadlines = self.deadlines;
        if let Some(error) = self.events.as_ref().and_then(Events::take_delivery_error) {
            failure.primary = "probe_cancel_wake_failed";
            failure.cleanup_error(error);
        }
        map_process_completion(
            &mut failure,
            self.parent_exited,
            self.assigned,
            self.job_empty,
        );
        failure.exit_code = self.exit_code.map(|code| code as i32);
        failure.stdout = (self.child_id != 0).then(|| self.stdout.data.metadata());
        failure.stderr = (self.child_id != 0).then(|| self.stderr.data.metadata());
        if let Some(cleanup_failure) = self.failure.take() {
            if cleanup_failure.primary == "probe_cancel_wake_failed" {
                failure.primary = cleanup_failure.primary;
            }
            failure.record_cleanup_errors(cleanup_failure.cleanup_errors);
            for resource in cleanup_failure.unconfirmed {
                failure.unconfirmed(resource);
            }
        }
        if self.failure_resources_settled() && !self.permanent_unconfirmed {
            if let Err(error) = self.release_reservation() {
                failure.cleanup_error(error);
                failure.unconfirmed(ResourceKind::OverlappedIo);
            }
        } else {
            self.mark_runtime_unconfirmed(&mut failure);
            self.failure = Some(failure.clone());
            if let Some(reservation) = self.reservation.take() {
                reservation.retain(Box::new(WindowsPending { run: self }));
            }
        }
        deadlines.record_lateness(&mut failure);
        Err(failure)
    }

    fn register(&mut self, kind: ResourceKind) -> bool {
        match self
            .reservation
            .as_mut()
            .map(Reservation::register_resource)
        {
            Some(Ok(())) => true,
            Some(Err(error)) => {
                self.record_owner_failure(format!("{}: {error}", resource_name(kind)));
                self.permanent_unconfirmed = true;
                false
            }
            None => false,
        }
    }

    pub fn settle_resource(&mut self, kind: ResourceKind, registered: bool) {
        if !registered {
            return;
        }
        if let Some(reservation) = self.reservation.as_mut() {
            if let Err(error) = reservation.resource_settled() {
                self.record_owner_failure(format!("{}: {error}", resource_name(kind)));
                self.permanent_unconfirmed = true;
            }
        }
    }

    fn fail(&mut self, primary: &'static str) {
        if self.failure.is_none() {
            self.failure = Some(ProbeFailure::new(primary));
        }
    }

    fn add_error(&mut self, error: impl AsRef<str>) {
        if let Some(failure) = self.failure.as_mut() {
            failure.cleanup_error(error);
        }
    }

    pub fn stream(&self, stdout: bool) -> &Stream {
        if stdout {
            &self.stdout
        } else {
            &self.stderr
        }
    }

    pub fn stream_mut(&mut self, stdout: bool) -> &mut Stream {
        if stdout {
            &mut self.stdout
        } else {
            &mut self.stderr
        }
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
}

fn map_process_completion(
    failure: &mut ProbeFailure,
    parent_exited: bool,
    assigned: bool,
    job_empty: bool,
) {
    failure.processes_confirmed = parent_exited && (!assigned || job_empty);
}

#[cfg(test)]
#[test]
fn capture_wait_failure_requires_both_process_and_job_completion() {
    let mut failure = ProbeFailure::new("probe_capture_or_completion_failed");
    failure.unconfirmed(ResourceKind::OverlappedIo);
    for (parent, assigned, empty, expected) in [
        (true, true, true, true),
        (true, true, false, false),
        (false, true, true, false),
        (true, false, false, true),
    ] {
        map_process_completion(&mut failure, parent, assigned, empty);
        assert_eq!(failure.processes_confirmed, expected);
        assert!(failure.is_unconfirmed());
        assert_eq!(failure.unconfirmed, vec![ResourceKind::OverlappedIo]);
    }
}
