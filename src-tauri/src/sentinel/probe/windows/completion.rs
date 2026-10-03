use super::*;

impl WindowsRun {
    pub(super) fn wait_parent_exit(&mut self, timeout: Option<Duration>) -> Result<bool, String> {
        let process = self
            .process
            .raw()
            .ok_or_else(|| "probe pending process handle missing".to_string())?;
        match unsafe { WaitForSingleObject(process, timeout.map(timeout_ms).unwrap_or(u32::MAX)) } {
            WAIT_OBJECT_0 => {
                self.observe_parent_exit()?;
                Ok(true)
            }
            WAIT_TIMEOUT => Ok(false),
            _ => Err(last_error()),
        }
    }

    pub(super) fn observe_parent_exit_if_signaled(&mut self) -> Result<(), String> {
        let Some(process) = self.process.raw() else {
            return Ok(());
        };
        match unsafe { WaitForSingleObject(process, 0) } {
            WAIT_OBJECT_0 => self.observe_parent_exit(),
            WAIT_TIMEOUT => Ok(()),
            _ => Err(last_error()),
        }
    }

    pub(super) fn observe_parent_exit(&mut self) -> Result<(), String> {
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
        self.close_completed_runtime_handles()
    }
}
