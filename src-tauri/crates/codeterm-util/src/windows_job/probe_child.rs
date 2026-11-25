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
        IO::{CancelIoEx, CreateIoCompletionPort, GetQueuedCompletionStatus, OVERLAPPED},
    },
};

const READ_CHUNK_BYTES: usize = 4096;
const TERMINATE_WAIT_MS: u32 = 5_000;
const STDOUT_KEY: usize = 1;
const JOB_KEY: usize = 2;
const JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO: u32 = 4;

static PIPE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub struct ProbeChild {
    process: OwnedHandle,
    job: Option<Job>,
    port: OwnedHandle,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadState {
    Idle,
    Active,
    Unsettled,
}

struct PinnedReadStorage {
    overlapped: OVERLAPPED,
    buffer: Vec<u8>,
    #[cfg(test)]
    dropped: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
}

#[cfg(test)]
impl Drop for PinnedReadStorage {
    fn drop(&mut self) {
        if let Some(flag) = &self.dropped {
            flag.store(true, Ordering::SeqCst);
        }
    }
}

struct IocpReader {
    port: HANDLE,
    pipe: Option<OwnedHandle>,
    state: ReadState,
    cancel_timeout_ms: u32,
    storage: Option<Box<PinnedReadStorage>>,
}

enum PipePeek {
    Available(u32),
    Broken,
    Error,
}

impl IocpReader {
    fn new(port: HANDLE, pipe: OwnedHandle) -> Self {
        Self {
            port,
            pipe: Some(pipe),
            state: ReadState::Idle,
            cancel_timeout_ms: TERMINATE_WAIT_MS,
            storage: Some(Box::new(PinnedReadStorage {
                overlapped: OVERLAPPED::default(),
                buffer: vec![0u8; READ_CHUNK_BYTES],
                #[cfg(test)]
                dropped: None,
            })),
        }
    }

    fn is_idle(&self) -> bool {
        self.state == ReadState::Idle
    }

    fn start_read(&mut self, remaining_cap: usize) -> io::Result<bool> {
        match self.state {
            ReadState::Idle => {}
            ReadState::Active => return Ok(false),
            ReadState::Unsettled => return Ok(true),
        }
        if remaining_cap == 0 {
            return Ok(true);
        }
        let Some(pipe) = self.pipe.as_ref() else {
            return Ok(true);
        };
        let Some(storage) = self.storage.as_deref_mut() else {
            return Ok(true);
        };
        let want = remaining_cap.min(storage.buffer.len()) as u32;
        storage.overlapped = OVERLAPPED::default();
        let ok = unsafe {
            ReadFile(
                pipe.as_raw_handle(),
                storage.buffer.as_mut_ptr(),
                want,
                null_mut(),
                &mut storage.overlapped,
            )
        };
        if ok != 0 {
            // Without FILE_SKIP_COMPLETION_PORT_ON_SUCCESS, a synchronous ReadFile
            // still queues its completion packet on the associated IOCP.
            self.state = ReadState::Active;
            Ok(false)
        } else {
            let err = unsafe { GetLastError() };
            if err == ERROR_IO_PENDING {
                self.state = ReadState::Active;
                Ok(false)
            } else if err == ERROR_BROKEN_PIPE {
                Ok(true)
            } else {
                Err(io::Error::from_raw_os_error(err as i32))
            }
        }
    }

    fn on_stdout_completion(
        &mut self,
        overlapped: *mut OVERLAPPED,
        bytes: u32,
        error: Option<u32>,
        output: &mut Vec<u8>,
    ) -> io::Result<bool> {
        let Some(storage) = self.storage.as_ref() else {
            return Err(io::Error::other("Probe stdout storage is unavailable"));
        };
        let expected: *const OVERLAPPED = &storage.overlapped;
        if self.state != ReadState::Active || overlapped.cast_const() != expected {
            return Err(io::Error::other("Unexpected probe stdout completion"));
        }
        self.state = ReadState::Idle;
        if let Some(err) = error {
            if err == ERROR_BROKEN_PIPE || err == ERROR_OPERATION_ABORTED {
                return Ok(true);
            }
            return Err(io::Error::from_raw_os_error(err as i32));
        }
        if bytes == 0 {
            return Ok(true);
        }
        let count = (bytes as usize).min(storage.buffer.len());
        output.extend_from_slice(&storage.buffer[..count]);
        Ok(false)
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

    fn cancel_into(
        &mut self,
        mut output: Option<&mut Vec<u8>>,
        mut job_empty: Option<&mut bool>,
    ) -> bool {
        if self.state != ReadState::Active {
            return false;
        }
        let Some(pipe) = self.pipe.as_ref() else {
            self.state = ReadState::Unsettled;
            return false;
        };
        let Some(storage) = self.storage.as_ref() else {
            self.state = ReadState::Unsettled;
            return false;
        };
        let expected: *const OVERLAPPED = &storage.overlapped;
        unsafe {
            CancelIoEx(pipe.as_raw_handle(), expected);
        }
        let mut harvested = false;
        loop {
            let mut bytes = 0u32;
            let mut key = 0usize;
            let mut overlapped: *mut OVERLAPPED = null_mut();
            let ok = unsafe {
                GetQueuedCompletionStatus(
                    self.port,
                    &mut bytes,
                    &mut key,
                    &mut overlapped,
                    self.cancel_timeout_ms,
                )
            };
            if ok == 0 && overlapped.is_null() {
                self.state = ReadState::Unsettled;
                break;
            }
            if key == JOB_KEY {
                if bytes == JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO {
                    if let Some(empty) = job_empty.as_deref_mut() {
                        *empty = true;
                    }
                }
                continue;
            }
            if key == STDOUT_KEY && overlapped.cast_const() == expected {
                if ok != 0 && bytes > 0 {
                    if let Some(buf) = output.as_deref_mut() {
                        let count = (bytes as usize).min(storage.buffer.len());
                        buf.extend_from_slice(&storage.buffer[..count]);
                        harvested = true;
                    }
                }
                self.state = ReadState::Idle;
                break;
            }
        }
        harvested
    }

    fn close(&mut self) {
        if self.state == ReadState::Active {
            self.cancel_into(None, None);
        }
        if self.state == ReadState::Idle {
            drop(self.pipe.take());
        }
    }
}

impl Drop for IocpReader {
    fn drop(&mut self) {
        if self.state == ReadState::Active {
            self.cancel_into(None, None);
        }
        if self.state != ReadState::Idle {
            // Retain unsettled overlapped storage rather than risk kernel use-after-free.
            if let Some(storage) = self.storage.take() {
                std::mem::forget(storage);
            }
        }
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
        let port = unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, null_mut(), 0, 1) };
        if port.is_null() {
            return Err(io::Error::last_os_error());
        }
        let port = unsafe { OwnedHandle::from_raw_handle(port) };
        let association = JOBOBJECT_ASSOCIATE_COMPLETION_PORT {
            CompletionKey: JOB_KEY as *mut std::ffi::c_void,
            CompletionPort: port.as_raw_handle(),
        };
        if unsafe {
            SetInformationJobObject(
                job.0 as HANDLE,
                JobObjectAssociateCompletionPortInformation,
                (&association as *const JOBOBJECT_ASSOCIATE_COMPLETION_PORT).cast(),
                std::mem::size_of_val(&association) as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let (stdout, writer) = create_overlapped_pipe()?;
        if unsafe {
            CreateIoCompletionPort(stdout.as_raw_handle(), port.as_raw_handle(), STDOUT_KEY, 0)
        }
        .is_null()
        {
            return Err(io::Error::last_os_error());
        }
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
            port,
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
        let mut reader = IocpReader::new(self.port.as_raw_handle(), stdout);
        let limit = max_bytes.saturating_add(1);
        let mut output = Vec::new();
        let mut pipe_done = false;
        let mut job_empty = self.job_is_empty();

        loop {
            if !pipe_done && reader.is_idle() {
                match reader.start_read(limit.saturating_sub(output.len())) {
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
                }
            }

            if pipe_done {
                return self
                    .wait_for_exit(deadline)
                    .map(|status| status.map(|status| (status, output)));
            }

            // Once every process in the probe's Job has exited, no probe process can write
            // more bytes even if an unrelated concurrent spawn inherited the write handle.
            if job_empty {
                match reader.peek() {
                    PipePeek::Broken => {
                        reader.close();
                        return self
                            .wait_for_exit(deadline)
                            .map(|status| status.map(|status| (status, output)));
                    }
                    PipePeek::Available(0) => {
                        if reader.cancel_into(Some(&mut output), Some(&mut job_empty)) {
                            if output.len() >= limit {
                                pipe_done = true;
                                reader.close();
                            }
                            continue;
                        }
                        reader.close();
                        return self
                            .wait_for_exit(deadline)
                            .map(|status| status.map(|status| (status, output)));
                    }
                    PipePeek::Available(_) | PipePeek::Error => {}
                }
            }

            let now = Instant::now();
            if now >= deadline {
                reader.close();
                self.terminate();
                return Ok(None);
            }
            let wait_ms = (deadline
                .saturating_duration_since(now)
                .as_millis()
                .min(u128::from(u32::MAX - 1)) as u32)
                .max(1);

            let mut bytes = 0u32;
            let mut key = 0usize;
            let mut overlapped: *mut OVERLAPPED = null_mut();
            let ok = unsafe {
                GetQueuedCompletionStatus(
                    self.port.as_raw_handle(),
                    &mut bytes,
                    &mut key,
                    &mut overlapped,
                    wait_ms,
                )
            };
            if ok == 0 && overlapped.is_null() {
                let err = unsafe { GetLastError() };
                reader.close();
                self.terminate();
                if err == WAIT_TIMEOUT {
                    return Ok(None);
                }
                return Err(io::Error::from_raw_os_error(err as i32));
            }
            if key == JOB_KEY {
                if bytes == JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO {
                    job_empty = true;
                }
                continue;
            }
            if key == STDOUT_KEY {
                let io_error = (ok == 0).then(|| unsafe { GetLastError() });
                match reader.on_stdout_completion(overlapped, bytes, io_error, &mut output) {
                    Ok(true) => {
                        pipe_done = true;
                        reader.close();
                    }
                    Ok(false) => {
                        if output.len() >= limit {
                            pipe_done = true;
                            reader.close();
                        }
                    }
                    Err(error) => {
                        reader.close();
                        self.terminate();
                        return Err(error);
                    }
                }
            }
        }
    }

    fn wait_for_exit(&mut self, deadline: Instant) -> io::Result<Option<ExitStatus>> {
        let wait_ms = deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(u128::from(u32::MAX - 1)) as u32;
        match unsafe { WaitForSingleObject(self.process.as_raw_handle(), wait_ms) } {
            WAIT_OBJECT_0 => self.exit_status().map(Some),
            WAIT_TIMEOUT => {
                self.terminate();
                Ok(None)
            }
            _ => {
                let error = io::Error::last_os_error();
                self.terminate();
                Err(error)
            }
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
