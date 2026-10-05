use super::*;

impl WindowsRun {
    pub(super) fn prepare_capture(&mut self) -> Result<(), String> {
        let nul = "NUL".encode_utf16().chain([0]).collect::<Vec<_>>();
        let mut inherit = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: null_mut(),
            bInheritHandle: 1,
        };
        let stdin = unsafe {
            CreateFileW(
                nul.as_ptr(),
                windows_sys::Win32::Foundation::GENERIC_READ,
                0,
                &inherit,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        };
        self.stdin = OwnedHandle::new(stdin).ok_or_else(last_error)?;
        self.stdin_registered = self.register(ResourceKind::Capture);
        if !self.stdin_registered {
            return Err("probe stdin ownership registration failed".into());
        }
        for stdout in [true, false] {
            self.prepare_stream(stdout, &mut inherit)?;
        }
        Ok(())
    }

    pub(super) fn prepare_stream(
        &mut self,
        stdout: bool,
        inherit: &mut SECURITY_ATTRIBUTES,
    ) -> Result<(), String> {
        let sequence = PIPE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let pipe_name = format!(
            "\\\\.\\pipe\\codeterm-sentinel-{}-{sequence}",
            std::process::id()
        );
        let wide = pipe_name.encode_utf16().chain([0]).collect::<Vec<_>>();
        let reader = unsafe {
            CreateNamedPipeW(
                wide.as_ptr(),
                PIPE_ACCESS_INBOUND | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                1,
                65_536,
                65_536,
                0,
                null(),
            )
        };
        self.stream_mut(stdout).reader = OwnedHandle::new(reader).ok_or_else(last_error)?;
        let registered = self.register(ResourceKind::Capture);
        self.stream_mut(stdout).capture_registered = registered;
        if !registered {
            return Err("probe pipe ownership registration failed".into());
        }
        let key = if stdout { STDOUT_KEY } else { STDERR_KEY };
        let reader = self.stream(stdout).reader.raw().unwrap();
        let associated =
            unsafe { CreateIoCompletionPort(reader, self.port.raw().unwrap(), key, 0) };
        if associated.is_null() {
            return Err(last_error());
        }
        let connect_registered = self.register(ResourceKind::OverlappedIo);
        self.stream_mut(stdout).connect_registered = connect_registered;
        if !connect_registered {
            return Err("probe pipe connection registration failed".into());
        }
        let connected =
            unsafe { ConnectNamedPipe(reader, &mut *self.stream_mut(stdout).connect_overlapped) };
        if connected != 0 {
            self.stream_mut(stdout).connect_pending = true;
        } else {
            let error = unsafe { GetLastError() };
            if error == ERROR_IO_PENDING {
                self.stream_mut(stdout).connect_pending = true;
            } else if error == ERROR_PIPE_CONNECTED {
                self.stream_mut(stdout).connected = true;
                let registered =
                    std::mem::replace(&mut self.stream_mut(stdout).connect_registered, false);
                self.settle_resource(ResourceKind::OverlappedIo, registered);
                let buffer_registered = std::mem::replace(
                    &mut self.stream_mut(stdout).connect_buffer_registered,
                    false,
                );
                self.settle_resource(ResourceKind::OverlappedIo, buffer_registered);
            } else {
                let registered =
                    std::mem::replace(&mut self.stream_mut(stdout).connect_registered, false);
                self.settle_resource(ResourceKind::OverlappedIo, registered);
                let buffer_registered = std::mem::replace(
                    &mut self.stream_mut(stdout).connect_buffer_registered,
                    false,
                );
                self.settle_resource(ResourceKind::OverlappedIo, buffer_registered);
                return Err(std::io::Error::from_raw_os_error(error as i32).to_string());
            }
        }
        let writer = unsafe {
            CreateFileW(
                wide.as_ptr(),
                windows_sys::Win32::Foundation::GENERIC_WRITE,
                0,
                inherit,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                null_mut(),
            )
        };
        self.stream_mut(stdout).child_writer = OwnedHandle::new(writer).ok_or_else(last_error)?;
        let registered = self.register(ResourceKind::Capture);
        self.stream_mut(stdout).writer_registered = registered;
        if !registered {
            return Err("probe pipe writer ownership registration failed".into());
        }
        let writer = self.stream(stdout).child_writer.raw().unwrap();
        if unsafe { SetHandleInformation(writer, HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT) } == 0 {
            return Err(last_error());
        }
        Ok(())
    }
}
