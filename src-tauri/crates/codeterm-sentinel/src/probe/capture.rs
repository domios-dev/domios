use std::process::ExitStatus;

use super::process::{ProbeFailure, ResourceKind};
use super::Output;

pub const PREFIX_BYTES: usize = 32 * 1024;

#[derive(Debug, Default)]
pub struct CaptureData {
    pub prefix: Vec<u8>,
    pub total_bytes: u64,
    pub truncated: bool,
    pub eof: bool,
}

impl CaptureData {
    pub fn append(&mut self, bytes: &[u8]) {
        self.total_bytes = self.total_bytes.saturating_add(bytes.len() as u64);
        let remaining = PREFIX_BYTES.saturating_sub(self.prefix.len());
        let retained = bytes.len().min(remaining);
        self.prefix.extend_from_slice(&bytes[..retained]);
        self.truncated |= retained < bytes.len();
    }

    pub fn metadata(&self) -> crate::command::Capture {
        crate::command::Capture {
            total_bytes: self.total_bytes,
            retained_bytes: self.prefix.len() as u64,
            truncated: self.truncated,
        }
    }

    pub fn mark_eof(&mut self) {
        self.eof = true;
    }
}

pub fn assemble(status: ExitStatus, stdout: CaptureData, stderr: CaptureData) -> Output {
    let status_code = status.code().unwrap_or(-1);
    let text = format!(
        "exit={}\nstdout:\n{}\nstderr:\n{}",
        status
            .code()
            .map(|code| code.to_string())
            .unwrap_or_else(|| "signal".into()),
        String::from_utf8_lossy(&stdout.prefix),
        String::from_utf8_lossy(&stderr.prefix)
    );
    let mut bytes = status_code.to_le_bytes().to_vec();
    bytes.extend_from_slice(&(stdout.prefix.len() as u64).to_le_bytes());
    bytes.extend_from_slice(&stdout.prefix);
    bytes.extend_from_slice(&stderr.prefix);
    Output {
        bytes,
        text,
        exit_code: status.code(),
        success: status.success(),
        stdout: stdout.metadata(),
        stderr: stderr.metadata(),
    }
}

pub struct CompletionFacts {
    pub parent_complete: bool,
    pub boundary_complete: bool,
    pub stdout_eof: bool,
    pub stderr_eof: bool,
    pub error: Option<&'static str>,
}

pub fn assemble_confirmed(
    status: ExitStatus,
    stdout: CaptureData,
    stderr: CaptureData,
    facts: CompletionFacts,
) -> Result<Output, ProbeFailure> {
    if let Some(error) = facts.error {
        let mut failure = ProbeFailure::new("probe_capture_or_completion_failed");
        failure.cleanup_error(error);
        failure.processes_confirmed = facts.parent_complete && facts.boundary_complete;
        if !facts.parent_complete {
            failure.unconfirmed(ResourceKind::Child);
        }
        if !facts.boundary_complete {
            failure.unconfirmed(ResourceKind::ProcessGroup);
        }
        failure.unconfirmed(ResourceKind::Capture);
        return Err(failure);
    }
    if !facts.parent_complete {
        let mut failure = ProbeFailure::new("probe_parent_completion_unconfirmed");
        failure.unconfirmed(ResourceKind::Child);
        return Err(failure);
    }
    if !facts.stdout_eof || !facts.stderr_eof || !stdout.eof || !stderr.eof {
        let mut failure = ProbeFailure::new("probe_capture_eof_unconfirmed");
        failure.processes_confirmed = facts.parent_complete && facts.boundary_complete;
        if !facts.parent_complete {
            failure.unconfirmed(ResourceKind::Child);
        }
        if !facts.boundary_complete {
            failure.unconfirmed(ResourceKind::ProcessGroup);
        }
        failure.unconfirmed(ResourceKind::Capture);
        return Err(failure);
    }
    if !facts.boundary_complete {
        let mut failure = ProbeFailure::new("probe_boundary_completion_unconfirmed");
        failure.unconfirmed(ResourceKind::ProcessGroup);
        return Err(failure);
    }
    Ok(assemble(status, stdout, stderr))
}

#[cfg(windows)]
pub struct OwnedHandle {
    raw: Option<windows_sys::Win32::Foundation::HANDLE>,
    uncertain: Option<windows_sys::Win32::Foundation::HANDLE>,
}

#[cfg(windows)]
impl OwnedHandle {
    pub fn empty() -> Self {
        Self {
            raw: None,
            uncertain: None,
        }
    }

    pub fn new(handle: windows_sys::Win32::Foundation::HANDLE) -> Option<Self> {
        use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
        (!handle.is_null() && handle != INVALID_HANDLE_VALUE).then_some(Self {
            raw: Some(handle),
            uncertain: None,
        })
    }

    pub fn raw(&self) -> Option<windows_sys::Win32::Foundation::HANDLE> {
        self.raw
    }

    pub fn is_open(&self) -> bool {
        self.raw.is_some() || self.uncertain.is_some()
    }

    pub fn close(&mut self) -> Result<(), String> {
        use windows_sys::Win32::Foundation::CloseHandle;
        if self.uncertain.is_some() {
            return Err("probe handle close completion is unconfirmed".into());
        }
        let Some(handle) = self.raw else {
            return Ok(());
        };
        if unsafe { CloseHandle(handle) } == 0 {
            self.raw = None;
            self.uncertain = Some(handle);
            Err(std::io::Error::last_os_error().to_string())
        } else {
            self.raw = None;
            Ok(())
        }
    }
}

#[cfg(windows)]
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        if let Some(handle) = self.raw.take() {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(handle);
            }
        }
    }
}

#[cfg(windows)]
pub struct PendingRead {
    pub overlapped: windows_sys::Win32::System::IO::OVERLAPPED,
    pub buffer: [u8; 4096],
}

#[cfg(windows)]
impl Default for PendingRead {
    fn default() -> Self {
        Self {
            overlapped: windows_sys::Win32::System::IO::OVERLAPPED::default(),
            buffer: [0; 4096],
        }
    }
}

#[cfg(windows)]
pub struct WindowsStream {
    pub reader: OwnedHandle,
    pub child_writer: OwnedHandle,
    pub connect_overlapped: Box<windows_sys::Win32::System::IO::OVERLAPPED>,
    pub pending: Box<PendingRead>,
    pub connect_pending: bool,
    pub pending_active: bool,
    pub connected: bool,
    pub io_registered: bool,
    pub connect_registered: bool,
    pub connect_buffer_registered: bool,
    pub buffer_registered: bool,
    pub capture_registered: bool,
    pub writer_registered: bool,
    pub eof: bool,
    pub read_failed: bool,
    pub data: CaptureData,
}

#[cfg(windows)]
impl WindowsStream {
    pub fn new() -> Self {
        Self {
            reader: OwnedHandle::empty(),
            child_writer: OwnedHandle::empty(),
            connect_overlapped: Box::new(windows_sys::Win32::System::IO::OVERLAPPED::default()),
            pending: Box::new(PendingRead::default()),
            connect_pending: false,
            pending_active: false,
            connected: false,
            io_registered: false,
            connect_registered: false,
            connect_buffer_registered: false,
            buffer_registered: false,
            capture_registered: false,
            writer_registered: false,
            eof: false,
            read_failed: false,
            data: CaptureData::default(),
        }
    }
}

#[cfg(windows)]
pub fn windows_command_line(
    command: &str,
    _events: Option<&super::cancellation::windows::Events>,
) -> Vec<u16> {
    #[cfg(any(test, feature = "test-support"))]
    if let Some(argv) = _events.and_then(super::cancellation::windows::Events::fixture_argv) {
        return windows_argument_line(argv.iter().map(String::as_str));
    }
    power_shell_command_line(command)
}

#[cfg(windows)]
pub fn power_shell_command_line(command: &str) -> Vec<u16> {
    windows_argument_line([
        "powershell.exe",
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        command,
    ])
}

#[cfg(windows)]
fn windows_argument_line<'a>(argv: impl IntoIterator<Item = &'a str>) -> Vec<u16> {
    argv.into_iter()
        .map(quote_windows_arg)
        .collect::<Vec<_>>()
        .join(" ")
        .encode_utf16()
        .chain([0])
        .collect()
}

#[cfg(windows)]
fn quote_windows_arg(argument: &str) -> String {
    if !argument.is_empty() && !argument.chars().any(|c| matches!(c, ' ' | '\t' | '"')) {
        return argument.into();
    }
    let mut result = String::from("\"");
    let mut slashes = 0;
    for character in argument.chars() {
        if character == '\\' {
            slashes += 1;
        } else if character == '"' {
            result.push_str(&"\\".repeat(slashes * 2 + 1));
            result.push('"');
            slashes = 0;
        } else {
            result.push_str(&"\\".repeat(slashes));
            result.push(character);
            slashes = 0;
        }
    }
    result.push_str(&"\\".repeat(slashes * 2));
    result.push('"');
    result
}

#[cfg(windows)]
pub fn timeout_ms(timeout: std::time::Duration) -> u32 {
    timeout
        .as_millis()
        .saturating_add(u128::from(timeout.subsec_nanos() % 1_000_000 != 0))
        .min(u32::MAX as u128 - 1) as u32
}

#[cfg(windows)]
pub fn last_error() -> String {
    std::io::Error::last_os_error().to_string()
}

#[cfg(windows)]
pub fn resource_name(kind: ResourceKind) -> &'static str {
    match kind {
        ResourceKind::Child => "child",
        ResourceKind::ProcessGroup => "process-group",
        ResourceKind::Capture => "capture",
        ResourceKind::Job => "job",
        ResourceKind::OverlappedIo => "overlapped-io",
    }
}

impl Output {
    /// Raw stdout of the command, read back from the layout `assemble` writes.
    pub fn stdout_text(&self) -> String {
        let len = self
            .bytes
            .get(4..12)
            .and_then(|raw| <[u8; 8]>::try_from(raw).ok())
            .map(u64::from_le_bytes)
            .unwrap_or(0) as usize;
        let end = 12usize.saturating_add(len).min(self.bytes.len());
        String::from_utf8_lossy(self.bytes.get(12..end).unwrap_or_default()).into_owned()
    }
}
