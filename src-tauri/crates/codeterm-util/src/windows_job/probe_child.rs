use super::*;
use std::{
    ffi::OsStr,
    fs::File,
    io,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::ExitStatusExt,
    },
    process::{Command, ExitStatus},
    ptr::{null, null_mut},
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};
use windows_sys::Win32::{
    Storage::FileSystem::{
        CreateFileW, ReadFile, FILE_ATTRIBUTE_NORMAL, FILE_FLAG_FIRST_PIPE_INSTANCE,
        FILE_FLAG_OVERLAPPED, OPEN_EXISTING, PIPE_ACCESS_INBOUND,
    },
    System::{
        Pipes::{
            CreateNamedPipeW, PeekNamedPipe, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_TYPE_BYTE, PIPE_WAIT,
        },
        IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED},
    },
};

const READ_CHUNK_BYTES: usize = 4096;
const JOB_DRAIN_SLICE_MS: u32 = 25;
const TERMINATE_WAIT_MS: u32 = 5_000;

static PIPE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub struct ProbeChild {
    process: OwnedHandle,
    job: Option<Job>,
    stdout: Option<OwnedHandle>,
}

struct Attributes(Vec<usize>);

impl Attributes {
    fn new(handles: &mut [HANDLE; 3], jobs: &mut [HANDLE; 1]) -> io::Result<Self> {
        let mut size = 0;
        unsafe { InitializeProcThreadAttributeList(null_mut(), 2, 0, &mut size) };
        if size == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut storage = vec![0; size.div_ceil(std::mem::size_of::<usize>())];
        let attributes = storage.as_mut_ptr().cast();
        if unsafe { InitializeProcThreadAttributeList(attributes, 2, 0, &mut size) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let owned = Self(storage);
        for (attribute, pointer, bytes) in [
            (
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
                handles.as_mut_ptr().cast(),
                std::mem::size_of_val(handles),
            ),
            (
                PROC_THREAD_ATTRIBUTE_JOB_LIST,
                jobs.as_mut_ptr().cast(),
                std::mem::size_of_val(jobs),
            ),
        ] {
            if unsafe {
                UpdateProcThreadAttribute(
                    attributes,
                    0,
                    attribute as usize,
                    pointer,
                    bytes,
                    null_mut(),
                    null(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        Ok(owned)
    }
}

impl Drop for Attributes {
    fn drop(&mut self) {
        unsafe { DeleteProcThreadAttributeList(self.0.as_mut_ptr().cast()) };
    }
}

struct OverlappedReader {
    pipe: Option<OwnedHandle>,
    event: OwnedHandle,
    active: bool,
    overlapped: Box<OVERLAPPED>,
    buffer: Vec<u8>,
}

enum PipePeek {
    Available(u32),
    Broken,
    Error,
}

impl OverlappedReader {
    fn new(pipe: OwnedHandle) -> io::Result<Self> {
        let event = unsafe { CreateEventW(null(), 1, 0, null()) };
        if event.is_null() {
            return Err(io::Error::last_os_error());
        }
        let event = unsafe { OwnedHandle::from_raw_handle(event) };
        Ok(Self {
            pipe: Some(pipe),
            event,
            active: false,
            overlapped: Box::new(OVERLAPPED::default()),
            buffer: vec![0u8; READ_CHUNK_BYTES],
        })
    }

    fn event_handle(&self) -> HANDLE {
        self.event.as_raw_handle()
    }

    fn start_or_drain(&mut self, output: &mut Vec<u8>, limit: usize) -> io::Result<bool> {
        while !self.active && output.len() < limit {
            let Some(pipe) = self.pipe.as_ref() else {
                return Ok(true);
            };
            let want = (limit - output.len()).min(self.buffer.len()) as u32;
            *self.overlapped = OVERLAPPED {
                hEvent: self.event.as_raw_handle(),
                ..OVERLAPPED::default()
            };
            let ok = unsafe {
                ReadFile(
                    pipe.as_raw_handle(),
                    self.buffer.as_mut_ptr(),
                    want,
                    null_mut(),
                    &mut *self.overlapped,
                )
            };
            if ok != 0 {
                let mut transferred = 0u32;
                if unsafe {
                    GetOverlappedResult(
                        pipe.as_raw_handle(),
                        &*self.overlapped,
                        &mut transferred,
                        0,
                    )
                } == 0
                {
                    let err = unsafe { GetLastError() };
                    if err == ERROR_BROKEN_PIPE {
                        return Ok(true);
                    }
                    return Err(io::Error::from_raw_os_error(err as i32));
                }
                if transferred == 0 {
                    return Ok(true);
                }
                output.extend_from_slice(&self.buffer[..transferred as usize]);
            } else {
                let err = unsafe { GetLastError() };
                if err == ERROR_IO_PENDING {
                    self.active = true;
                    return Ok(false);
                }
                if err == ERROR_BROKEN_PIPE {
                    return Ok(true);
                }
                return Err(io::Error::from_raw_os_error(err as i32));
            }
        }
        Ok(output.len() >= limit)
    }

    fn harvest_if_ready(&mut self, output: &mut Vec<u8>) -> io::Result<Option<bool>> {
        if !self.active {
            return Ok(None);
        }
        if unsafe { WaitForSingleObject(self.event.as_raw_handle(), 0) } != WAIT_OBJECT_0 {
            return Ok(None);
        }
        self.harvest_completed(output).map(Some)
    }

    fn harvest_completed(&mut self, output: &mut Vec<u8>) -> io::Result<bool> {
        let Some(pipe) = self.pipe.as_ref() else {
            self.active = false;
            return Ok(true);
        };
        let mut transferred = 0u32;
        let ok = unsafe {
            GetOverlappedResult(pipe.as_raw_handle(), &*self.overlapped, &mut transferred, 0)
        };
        self.active = false;
        if ok != 0 {
            if transferred == 0 {
                return Ok(true);
            }
            output.extend_from_slice(&self.buffer[..transferred as usize]);
            Ok(false)
        } else {
            let err = unsafe { GetLastError() };
            if err == ERROR_BROKEN_PIPE || err == ERROR_OPERATION_ABORTED {
                Ok(true)
            } else {
                Err(io::Error::from_raw_os_error(err as i32))
            }
        }
    }

    fn peek(&self) -> PipePeek {
        let Some(pipe) = self.pipe.as_ref() else {
            return PipePeek::Broken;
        };
        let mut available = 0u32;
        if unsafe {
            PeekNamedPipe(
                pipe.as_raw_handle(),
                null_mut(),
                0,
                null_mut(),
                &mut available,
                null_mut(),
            )
        } != 0
        {
            PipePeek::Available(available)
        } else if unsafe { GetLastError() } == ERROR_BROKEN_PIPE {
            PipePeek::Broken
        } else {
            PipePeek::Error
        }
    }

    fn cancel_into(&mut self, output: Option<&mut Vec<u8>>) -> bool {
        if !self.active {
            return false;
        }
        let mut harvested = false;
        if let Some(pipe) = self.pipe.as_ref() {
            unsafe {
                CancelIoEx(pipe.as_raw_handle(), &*self.overlapped);
                let mut transferred = 0u32;
                if GetOverlappedResult(pipe.as_raw_handle(), &*self.overlapped, &mut transferred, 1)
                    != 0
                    && transferred > 0
                {
                    if let Some(output) = output {
                        output.extend_from_slice(&self.buffer[..transferred as usize]);
                        harvested = true;
                    }
                }
            }
        }
        self.active = false;
        harvested
    }

    fn close(&mut self) {
        self.cancel_into(None);
        drop(self.pipe.take());
    }
}

impl Drop for OverlappedReader {
    fn drop(&mut self) {
        self.cancel_into(None);
    }
}

fn create_overlapped_pipe() -> io::Result<(OwnedHandle, OwnedHandle)> {
    let sequence = PIPE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let name = format!(
        r"\\.\pipe\codeterm-probe-child-{}-{sequence}",
        std::process::id()
    );
    let mut wide_name = wide(OsStr::new(&name))?;
    wide_name.push(0);
    let reader = unsafe {
        CreateNamedPipeW(
            wide_name.as_ptr(),
            PIPE_ACCESS_INBOUND | FILE_FLAG_OVERLAPPED | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            65_536,
            65_536,
            0,
            null(),
        )
    };
    if reader == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let reader = unsafe { OwnedHandle::from_raw_handle(reader) };
    let writer = unsafe {
        CreateFileW(
            wide_name.as_ptr(),
            GENERIC_WRITE,
            0,
            null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    };
    if writer == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    let writer = unsafe { OwnedHandle::from_raw_handle(writer) };
    Ok((reader, writer))
}

fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
    let units: Vec<_> = value.encode_wide().collect();
    if units.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "NUL in command",
        ));
    }
    Ok(units)
}

fn argument(value: &OsStr, output: &mut Vec<u16>) -> io::Result<()> {
    output.push(b'"' as u16);
    let mut slashes = 0;
    for unit in wide(value)? {
        if unit == b'\\' as u16 {
            slashes += 1;
            continue;
        }
        output.extend(std::iter::repeat_n(
            b'\\' as u16,
            if unit == b'"' as u16 {
                slashes * 2 + 1
            } else {
                slashes
            },
        ));
        output.push(unit);
        slashes = 0;
    }
    output.extend(std::iter::repeat_n(b'\\' as u16, slashes * 2));
    output.push(b'"' as u16);
    Ok(())
}

impl ProbeChild {
    pub fn spawn(command: &mut Command) -> io::Result<Self> {
        Self::spawn_inner(command, false).map(|(child, _)| child)
    }

    fn spawn_inner(
        command: &mut Command,
        retain_writer_copy: bool,
    ) -> io::Result<(Self, Option<OwnedHandle>)> {
        if command.get_program().is_empty() || command.get_envs().next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid probe command",
            ));
        }
        let binary = std::path::Path::new(command.get_program());
        let batch = binary
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"));
        let program = if batch {
            std::env::var_os("SystemRoot")
                .map(|root| std::path::PathBuf::from(root).join("System32/cmd.exe"))
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "SystemRoot is missing"))?
        } else {
            binary.to_path_buf()
        };
        let mut line = Vec::new();
        argument(program.as_os_str(), &mut line)?;
        if batch {
            if wide(binary.as_os_str())?
                .iter()
                .any(|unit| [b'%' as u16, b'"' as u16, 10, 13].contains(unit))
                || command.get_args().any(|arg| arg != "--version")
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Unsafe batch probe command",
                ));
            }
            line.extend(" /d /v:off /s /c \"".encode_utf16());
            argument(binary.as_os_str(), &mut line)?;
        }
        for arg in command.get_args() {
            line.push(b' ' as u16);
            argument(arg, &mut line)?;
        }
        if batch {
            line.push(b'"' as u16);
        }
        line.push(0);
        let mut program = wide(program.as_os_str())?;
        program.push(0);
        let cwd = command
            .get_current_dir()
            .map(|path| {
                let mut path = wide(path.as_os_str())?;
                path.push(0);
                Ok::<_, io::Error>(path)
            })
            .transpose()?;
        let job = Job::new().map_err(io::Error::other)?;
        let (stdout, writer) = create_overlapped_pipe()?;
        let leaked_writer = if retain_writer_copy {
            Some(writer.try_clone()?)
        } else {
            None
        };
        let input = File::open(r"\\.\NUL")?;
        let error = File::options().write(true).open(r"\\.\NUL")?;
        let mut handles = [
            input.as_raw_handle(),
            writer.as_raw_handle(),
            error.as_raw_handle(),
        ];
        let mut jobs = [job.0 as HANDLE];
        let mut attributes = Attributes::new(&mut handles, &mut jobs)?;
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = handles[0];
        startup.StartupInfo.hStdOutput = handles[1];
        startup.StartupInfo.hStdError = handles[2];
        startup.lpAttributeList = attributes.0.as_mut_ptr().cast();
        let mut information = PROCESS_INFORMATION::default();
        // Only the listed child ends are inheritable, and only across CreateProcessW.
        for handle in handles {
            if unsafe {
                SetHandleInformation(handle.cast(), HANDLE_FLAG_INHERIT, HANDLE_FLAG_INHERIT)
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
        }
        // Job assignment is atomic with creation, including owner death before this call returns.
        let created = unsafe {
            CreateProcessW(
                program.as_ptr(),
                line.as_mut_ptr(),
                null(),
                null(),
                1,
                CREATE_NO_WINDOW | CREATE_SUSPENDED | EXTENDED_STARTUPINFO_PRESENT,
                null(),
                cwd.as_ref().map_or(null(), |path| path.as_ptr()),
                &startup.StartupInfo,
                &mut information,
            )
        };
        let create_error = (created == 0).then(io::Error::last_os_error);
        drop(input);
        drop(writer);
        drop(error);
        if let Some(error) = create_error {
            return Err(error);
        }
        let process = unsafe { OwnedHandle::from_raw_handle(information.hProcess) };
        let thread = unsafe { OwnedHandle::from_raw_handle(information.hThread) };
        let child = Self {
            process,
            job: Some(job),
            stdout: Some(stdout),
        };
        if unsafe { ResumeThread(thread.as_raw_handle()) } == u32::MAX {
            return Err(io::Error::last_os_error());
        }
        Ok((child, leaked_writer))
    }

    pub fn collect_output(
        &mut self,
        deadline: Instant,
        max_bytes: usize,
    ) -> io::Result<Option<(ExitStatus, Vec<u8>)>> {
        let Some(stdout) = self.stdout.take() else {
            self.terminate();
            return Err(io::Error::other("Probe stdout is unavailable"));
        };
        let mut reader = match OverlappedReader::new(stdout) {
            Ok(reader) => reader,
            Err(error) => {
                self.terminate();
                return Err(error);
            }
        };
        let limit = max_bytes.saturating_add(1);
        let mut output = Vec::new();
        let mut pipe_done = false;
        let mut exit_status: Option<ExitStatus> = None;

        loop {
            if !pipe_done {
                if let Some(done) = match reader.harvest_if_ready(&mut output) {
                    Ok(done) => done,
                    Err(error) => {
                        reader.close();
                        self.terminate();
                        return Err(error);
                    }
                } {
                    pipe_done = done;
                }
            }
            if !pipe_done {
                match reader.start_or_drain(&mut output, limit) {
                    Ok(true) => pipe_done = true,
                    Ok(false) => {}
                    Err(error) => {
                        reader.close();
                        self.terminate();
                        return Err(error);
                    }
                }
            }
            if pipe_done {
                reader.close();
            }
            if exit_status.is_none() {
                match self.try_wait() {
                    Ok(Some(status)) => exit_status = Some(status),
                    Ok(None) => {}
                    Err(error) => {
                        reader.close();
                        self.terminate();
                        return Err(error);
                    }
                }
            }
            if let Some(status) = exit_status {
                if pipe_done {
                    return Ok(Some((status, output)));
                }
                // Once every process in the probe's Job has exited, no probe process can write
                // more bytes even if an unrelated concurrent spawn inherited the write handle.
                if self.job_is_empty() {
                    match reader.peek() {
                        PipePeek::Broken => {
                            reader.close();
                            return Ok(Some((status, output)));
                        }
                        PipePeek::Available(0) => {
                            if reader.cancel_into(Some(&mut output)) {
                                continue;
                            }
                            reader.close();
                            return Ok(Some((status, output)));
                        }
                        PipePeek::Available(_) | PipePeek::Error => {}
                    }
                }
            }
            let now = Instant::now();
            if now >= deadline {
                reader.close();
                self.terminate();
                return Ok(None);
            }
            let wait_ms = deadline
                .saturating_duration_since(now)
                .as_millis()
                .min(u128::from(u32::MAX - 1)) as u32;
            if exit_status.is_none() {
                if reader.active {
                    let handles = [reader.event_handle(), self.process.as_raw_handle()];
                    match unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, wait_ms) } {
                        WAIT_OBJECT_0 => match reader.harvest_completed(&mut output) {
                            Ok(true) => {
                                pipe_done = true;
                                reader.close();
                            }
                            Ok(false) => {}
                            Err(error) => {
                                reader.close();
                                self.terminate();
                                return Err(error);
                            }
                        },
                        val if val == WAIT_OBJECT_0 + 1 => match self.exit_status() {
                            Ok(status) => exit_status = Some(status),
                            Err(error) => {
                                reader.close();
                                self.terminate();
                                return Err(error);
                            }
                        },
                        WAIT_TIMEOUT => {
                            reader.close();
                            self.terminate();
                            return Ok(None);
                        }
                        _ => {
                            let error = io::Error::last_os_error();
                            reader.close();
                            self.terminate();
                            return Err(error);
                        }
                    }
                } else {
                    match unsafe { WaitForSingleObject(self.process.as_raw_handle(), wait_ms) } {
                        WAIT_OBJECT_0 => match self.exit_status() {
                            Ok(status) => exit_status = Some(status),
                            Err(error) => {
                                reader.close();
                                self.terminate();
                                return Err(error);
                            }
                        },
                        WAIT_TIMEOUT => {
                            reader.close();
                            self.terminate();
                            return Ok(None);
                        }
                        _ => {
                            let error = io::Error::last_os_error();
                            reader.close();
                            self.terminate();
                            return Err(error);
                        }
                    }
                }
            } else if reader.active {
                let slice_ms = wait_ms.min(JOB_DRAIN_SLICE_MS);
                match unsafe { WaitForSingleObject(reader.event_handle(), slice_ms) } {
                    WAIT_OBJECT_0 => match reader.harvest_completed(&mut output) {
                        Ok(true) => {
                            pipe_done = true;
                            reader.close();
                        }
                        Ok(false) => {}
                        Err(error) => {
                            reader.close();
                            self.terminate();
                            return Err(error);
                        }
                    },
                    WAIT_TIMEOUT => {}
                    _ => {
                        let error = io::Error::last_os_error();
                        reader.close();
                        self.terminate();
                        return Err(error);
                    }
                }
            }
        }
    }

    fn try_wait(&self) -> io::Result<Option<ExitStatus>> {
        match unsafe { WaitForSingleObject(self.process.as_raw_handle(), 0) } {
            WAIT_OBJECT_0 => self.exit_status().map(Some),
            WAIT_TIMEOUT => Ok(None),
            _ => Err(io::Error::last_os_error()),
        }
    }

    fn exit_status(&self) -> io::Result<ExitStatus> {
        let mut code = 0;
        if unsafe { GetExitCodeProcess(self.process.as_raw_handle(), &mut code) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(ExitStatus::from_raw(code))
    }

    fn job_is_empty(&self) -> bool {
        self.job
            .as_ref()
            .and_then(|job| job.active_processes().ok())
            .is_some_and(|active| active == 0)
    }

    pub fn terminate(&mut self) {
        if let Some(job) = &self.job {
            if job.try_terminate().is_err() {
                unsafe { TerminateProcess(self.process.as_raw_handle(), 1) };
            }
        }
        unsafe { WaitForSingleObject(self.process.as_raw_handle(), TERMINATE_WAIT_MS) };
    }
}

impl Drop for ProbeChild {
    fn drop(&mut self) {
        drop(self.job.take());
        unsafe { WaitForSingleObject(self.process.as_raw_handle(), TERMINATE_WAIT_MS) };
    }
}

#[cfg(test)]
mod tests;
