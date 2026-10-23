use super::*;

pub(super) fn assign_process(job: HANDLE, process: HANDLE) -> Result<(), String> {
    if unsafe { AssignProcessToJobObject(job, process) } == 0 {
        Err(last_error())
    } else {
        Ok(())
    }
}

impl WindowsRun {
    pub(super) fn spawn_contained(
        &mut self,
        command: &str,
        cwd: &Path,
        environment: &Environment,
        executable: Option<Executable<'_>>,
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
        let mut command_line = executable.as_ref().map_or_else(
            || capture::windows_command_line(command, self.events.as_ref()),
            capture::executable_command_line,
        );
        let program = executable.as_ref().map(|executable| {
            executable
                .program
                .as_os_str()
                .encode_wide()
                .chain([0])
                .collect::<Vec<_>>()
        });
        let mut current_directory = cwd.as_os_str().encode_wide().chain([0]).collect::<Vec<_>>();
        let mut environment_block = environment.windows_block();
        let environment_pointer = environment_block.as_mut_ptr().cast();
        let creation_flags = CREATE_SUSPENDED
            | CREATE_NO_WINDOW
            | EXTENDED_STARTUPINFO_PRESENT
            | CREATE_UNICODE_ENVIRONMENT;
        let mut information = windows_sys::Win32::System::Threading::PROCESS_INFORMATION::default();
        if self.cancelled() {
            unsafe { DeleteProcThreadAttributeList(attributes) };
            self.fail("probe_cancelled");
            return Err("probe_cancelled".into());
        }
        let created = self.deadlines.spawn_with(|| unsafe {
            CreateProcessW(
                program.as_ref().map_or(null(), |program| program.as_ptr()),
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
        } != 0, Instant::now);
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
}
