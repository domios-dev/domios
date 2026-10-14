use super::*;

impl WindowsRun {
    pub(in crate::probe) fn release_process_admission(&mut self) {
        if self.parent_exited && (!self.assigned || self.job_empty) {
            if let Some(reservation) = self.reservation.as_mut() {
                reservation.release_process_admission();
            }
        }
    }

    pub(in crate::probe) fn reconcile_retained(&mut self) -> bool {
        self.settle_retained_capture(false)
    }

    pub(in crate::probe) fn wait_for_retained_capture(&mut self) -> bool {
        self.settle_retained_capture(true)
    }

    pub(in crate::probe) fn retained_processes_settled(&mut self) -> bool {
        let parent_settled = self.child_id == 0
            || self.parent_exited
            || self.process.raw().is_some_and(|process| {
                (unsafe { WaitForSingleObject(process, 0) }) == WAIT_OBJECT_0
            });
        if self.assigned && !self.job_empty {
            if let Err(error) = self.observe_job_accounting() {
                self.record_retained_cleanup_error(error);
            }
        }
        parent_settled && (!self.assigned || self.job_empty)
    }

    fn settle_retained_capture(&mut self, wait: bool) -> bool {
        let result = self.reconcile_retained_handles(wait);
        if let Err(error) = result {
            self.record_retained_cleanup_error(error);
        }
        self.failure_resources_settled()
    }

    fn reconcile_retained_handles(&mut self, wait: bool) -> Result<(), String> {
        if let Err(error) = self.observe_parent_exit_if_signaled() {
            self.record_retained_cleanup_error(error);
        }
        if self.assigned && !self.job_empty {
            self.observe_job_accounting()?;
        }
        for stdout in [true, false] {
            if self.stream(stdout).connect_pending && self.stream(stdout).pending_active {
                return Err("probe pipe has overlapping connection and read ownership".into());
            }
            for connect in [true, false] {
                let stream = self.stream_mut(stdout);
                let pending = if connect {
                    stream.connect_pending
                } else {
                    stream.pending_active
                };
                if !pending {
                    continue;
                }
                let reader = stream
                    .reader
                    .raw()
                    .ok_or("probe pending I/O handle missing")?;
                let overlapped = if connect {
                    &*stream.connect_overlapped
                } else {
                    &stream.pending.overlapped
                };
                let mut bytes = 0;
                let completed = unsafe {
                    windows_sys::Win32::System::IO::GetOverlappedResult(
                        reader,
                        overlapped,
                        &mut bytes,
                        i32::from(wait),
                    )
                } != 0;
                if !completed {
                    match unsafe { GetLastError() } {
                        windows_sys::Win32::Foundation::ERROR_IO_INCOMPLETE => continue,
                        ERROR_OPERATION_ABORTED | ERROR_BROKEN_PIPE => {}
                        error => {
                            return Err(native_error("GetOverlappedResult", Some(stdout), error))
                        }
                    }
                }
                let registered = if connect {
                    stream.connect_pending = false;
                    std::mem::replace(&mut stream.connect_registered, false)
                } else {
                    stream.pending_active = false;
                    std::mem::replace(&mut stream.io_registered, false)
                };
                self.settle_resource(ResourceKind::OverlappedIo, registered);
            }
            if !self.stream(stdout).connect_pending && !self.stream(stdout).pending_active {
                self.close_reader(stdout)?;
            }
        }
        self.settle_idle_buffers();
        if !self.assigned && !self.process.is_open() {
            self.close_unassigned_resources()?;
        }
        self.close_completed_runtime_handles()
    }
}

pub fn native_error(operation: &str, stdout: Option<bool>, error: u32) -> String {
    let stream = match stdout {
        Some(true) => "stdout",
        Some(false) => "stderr",
        None => "completion-port",
    };
    format!("{operation} {stream} win32_error={error}")
}

impl WindowsRun {
    #[cfg(test)]
    pub fn trace_native(
        &mut self,
        operation: &'static str,
        stdout: Option<bool>,
        bytes: Option<u32>,
        error: Option<u32>,
    ) {
        let unexpected_error =
            error.is_some_and(|error| !matches!(error, ERROR_IO_PENDING | ERROR_BROKEN_PIPE));
        if self.native_trace_remaining == 0 && (!unexpected_error || self.native_error_reported) {
            return;
        }
        self.native_error_reported |= unexpected_error;
        self.native_trace_remaining = self.native_trace_remaining.saturating_sub(1);
        if let Some(events) = &self.events {
            events.waiting(super::super::cancellation::WaitPhase::WindowsIo {
                operation,
                stdout,
                bytes,
                error,
                pending: stdout.map(|stdout| self.stream(stdout).pending_active),
                eof: stdout.map(|stdout| self.stream(stdout).eof),
            });
        }
    }

    pub fn wait_parent_exit(&mut self, timeout: Option<Duration>) -> Result<bool, String> {
        let process = self
            .process
            .raw()
            .ok_or_else(|| "probe pending process handle missing".to_string())?;
        #[cfg(test)]
        if let Some(events) = &self.events {
            use super::super::cancellation::WaitPhase;
            if self.stdout.eof
                && self.stderr.eof
                && !self.cancel_attempted
                && unsafe { WaitForSingleObject(process, 0) } == WAIT_TIMEOUT
            {
                events.waiting(WaitPhase::ParentAfterEof);
            }
            events.waiting(WaitPhase::Parent);
        }
        let result = if !self.cancel_attempted {
            if let Some(cancel) = self.events.as_ref().and_then(Events::event) {
                let handles = [process, cancel];
                unsafe {
                    windows_sys::Win32::System::Threading::WaitForMultipleObjects(
                        2,
                        handles.as_ptr(),
                        0,
                        timeout.map(timeout_ms).unwrap_or(u32::MAX),
                    )
                }
            } else {
                unsafe { WaitForSingleObject(process, timeout.map(timeout_ms).unwrap_or(u32::MAX)) }
            }
        } else {
            unsafe { WaitForSingleObject(process, timeout.map(timeout_ms).unwrap_or(u32::MAX)) }
        };
        if result == WAIT_OBJECT_0 + 1 {
            self.fail("probe_cancelled");
            return Ok(true);
        }
        match result {
            WAIT_OBJECT_0 => {
                self.observe_parent_exit()?;
                Ok(true)
            }
            WAIT_TIMEOUT => Ok(false),
            _ => Err(last_error()),
        }
    }

    pub fn observe_parent_exit_if_signaled(&mut self) -> Result<(), String> {
        let Some(process) = self.process.raw() else {
            return Ok(());
        };
        match unsafe { WaitForSingleObject(process, 0) } {
            WAIT_OBJECT_0 => self.observe_parent_exit(),
            WAIT_TIMEOUT => Ok(()),
            _ => Err(last_error()),
        }
    }

    pub fn observe_parent_exit(&mut self) -> Result<(), String> {
        let Some(process) = self.process.raw() else {
            return Ok(());
        };
        if unsafe { WaitForSingleObject(process, 0) } != WAIT_OBJECT_0 {
            return Err("Job exit packet preceded captured process signaling".into());
        }
        let mut exit_code = 0u32;
        if unsafe { GetExitCodeProcess(process, &mut exit_code) } == 0 {
            return Err(last_error());
        }
        self.exit_code = Some(exit_code);
        self.parent_exited = true;
        self.parent_exit_pending = false;
        self.process.close()?;
        let registered = std::mem::replace(&mut self.process_registered, false);
        self.settle_resource(ResourceKind::Child, registered);
        if self.primary_thread.is_open() {
            self.close_thread_handle()?;
        }
        // Job accounting retains terminated processes until their references are released.
        if self.assigned && !self.job_empty {
            self.observe_job_accounting()?;
        }
        self.release_process_admission();
        self.close_completed_runtime_handles()
    }
}

impl WindowsRun {
    pub fn prepare_cancellation(&mut self, signal: &Cancellation) -> Result<(), String> {
        self.events = Some(Events::new(signal)?);
        self.events_registered = self.register(ResourceKind::OverlappedIo);
        if self.events_registered {
            Ok(())
        } else {
            Err("probe cancellation event registration failed".into())
        }
    }

    pub fn cancelled(&self) -> bool {
        self.events.as_ref().is_some_and(Events::cancelled)
    }

    pub(in crate::probe) fn detach_cancel_port(&self) {
        if let Some(events) = &self.events {
            events.detach_port();
        }
    }

    pub(in crate::probe) fn close_cancellation(&mut self) -> Result<(), String> {
        if let Some(error) = self.events.as_ref().and_then(Events::take_delivery_error) {
            self.fail("probe_cancel_wake_failed");
            self.add_error(error);
        }
        if let Some(events) = &mut self.events {
            events.close()?;
            self.events = None;
            let registered = std::mem::replace(&mut self.events_registered, false);
            self.settle_resource(ResourceKind::OverlappedIo, registered);
        }
        Ok(())
    }
}

impl WindowsRun {
    pub(in crate::probe) fn wait_event(
        &mut self,
        timeout: Option<Duration>,
    ) -> Result<bool, String> {
        if self.parent_exit_pending
            || (self.stdout.eof && self.stderr.eof && self.process.is_open())
        {
            return self.wait_parent_exit(timeout);
        }
        let Some(port) = self.port.raw() else {
            return Err("probe completion port is not owned".into());
        };
        let mut bytes = 0u32;
        let mut key = 0usize;
        let mut overlapped = null_mut();
        let millis = timeout.map(timeout_ms).unwrap_or(u32::MAX);
        #[cfg(test)]
        if let Some(events) = &self.events {
            use super::super::cancellation::WaitPhase;
            events.waiting(WaitPhase::WindowsCapture {
                stdout_eof: self.stdout.eof,
                stderr_eof: self.stderr.eof,
                stdout_pending: self.stdout.pending_active,
                stderr_pending: self.stderr.pending_active,
                parent_exited: self.parent_exited,
                cancel_attempted: self.cancel_attempted,
            });
            events.waiting(WaitPhase::Capture);
        }
        let succeeded = unsafe {
            GetQueuedCompletionStatus(port, &mut bytes, &mut key, &mut overlapped, millis)
        } != 0;
        let event_error = if succeeded {
            None
        } else {
            Some(unsafe { GetLastError() })
        };
        if !succeeded && overlapped.is_null() {
            #[cfg(test)]
            self.trace_native("GetQueuedCompletionStatus", None, None, event_error);
            if event_error == Some(WAIT_TIMEOUT) {
                return Ok(false);
            }
            self.permanent_unconfirmed = true;
            return Err(event_error
                .map(|error| native_error("GetQueuedCompletionStatus", None, error))
                .unwrap_or_else(|| "probe completion packet lacked an owned event".into()));
        }
        if key == CANCEL_KEY && succeeded && overlapped.is_null() {
            if !self.cancel_attempted {
                self.fail("probe_cancelled");
            }
        } else if key == JOB_KEY {
            if let Err(error) = self.observe_job_event(bytes, overlapped as usize as u32) {
                self.permanent_unconfirmed = true;
                return Err(error);
            }
        } else if key == STDOUT_KEY || key == STDERR_KEY {
            if overlapped.is_null() {
                self.unknown_completion = true;
                self.permanent_unconfirmed = true;
                return Err("probe capture completion lacked an owned operation".into());
            }
            if let Err(error) =
                self.observe_read_event(key == STDOUT_KEY, overlapped, bytes, event_error)
            {
                if self.unknown_completion {
                    self.permanent_unconfirmed = true;
                }
                return Err(error);
            }
            if let Err(error) = self.close_completed_runtime_handles() {
                self.permanent_unconfirmed = true;
                return Err(error);
            }
        } else {
            self.unknown_completion = true;
            self.permanent_unconfirmed = true;
            return Err("probe completion packet had an unknown owner key".into());
        }
        Ok(true)
    }
}
