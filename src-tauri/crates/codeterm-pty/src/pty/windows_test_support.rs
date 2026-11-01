use std::collections::HashSet;
use std::io::Read;
use std::process::Stdio;
use std::time::{Duration, Instant};

use wait_timeout::ChildExt;
mod diagnostics;
pub(super) use diagnostics::print_pty_environment;
use windows_sys::Win32::Foundation::{
    CloseHandle, DuplicateHandle, DUPLICATE_SAME_ACCESS, ERROR_NO_MORE_FILES, HANDLE,
    INVALID_HANDLE_VALUE, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE,
};

// Hosts are attributed by parent PID, which is the whole test process, so a
// concurrent test's ConPTY would miscount as this one's. Hold it while observing.
pub(super) fn exclusive_conpty_test() -> parking_lot::MutexGuard<'static, ()> {
    static LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
    LOCK.lock()
}

pub(super) fn legacy_conhost_children(parent_pid: u32) -> Result<Vec<u32>, String> {
    let script = format!(
        "$p={}; Get-CimInstance Win32_Process -Filter \"Name = 'conhost.exe' AND ParentProcessId = $p\" | Where-Object {{ $_.CommandLine -like '*conhost.exe 0x4*' }} | ForEach-Object {{ $_.ProcessId }}",
        parent_pid
    );
    Ok(run_process_query(&script)?.into_iter().collect())
}

pub(super) fn conpty_host_children_of_current_process() -> Result<HashSet<u32>, String> {
    direct_console_children(std::process::id())
}

pub(super) fn new_conpty_hosts(before: &HashSet<u32>) -> Result<Vec<u32>, String> {
    Ok(conpty_host_children_of_current_process()?
        .difference(before)
        .copied()
        .collect())
}

pub(super) struct ProcessExitWitness {
    pid: u32,
    role: &'static str,
    handle: OwnedHandle,
}

impl ProcessExitWitness {
    pub(super) fn for_child(child: &dyn portable_pty::Child) -> Result<Self, String> {
        let pid = child.process_id().ok_or("PTY child has no process ID")?;
        let source = child
            .as_raw_handle()
            .ok_or_else(|| format!("PTY child {pid} has no process handle"))?;
        let mut duplicate = std::ptr::null_mut();
        let process = unsafe { GetCurrentProcess() };
        if unsafe {
            DuplicateHandle(
                process,
                source.cast(),
                process,
                &mut duplicate,
                0,
                0,
                DUPLICATE_SAME_ACCESS,
            )
        } == 0
        {
            return Err(last_error(&format!(
                "duplicate PTY child {pid} process handle"
            )));
        }
        Ok(Self {
            pid,
            role: "PTY child",
            handle: OwnedHandle::new(duplicate, "capture PTY child process handle")?,
        })
    }

    fn wait_until(&self, deadline: Instant) -> Result<(), String> {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let milliseconds = remaining.as_millis().min(u32::MAX as u128 - 1) as u32;
        match unsafe { WaitForSingleObject(self.handle.0, milliseconds) } {
            WAIT_OBJECT_0 => Ok(()),
            WAIT_TIMEOUT => Err(format!(
                "{} {} remains alive at the shared process-exit deadline",
                self.role, self.pid
            )),
            WAIT_FAILED => Err(last_error(&format!(
                "observe {} {} process exit",
                self.role, self.pid
            ))),
            result => Err(format!(
                "observe {} {} process exit: unexpected wait result {result:#x}",
                self.role, self.pid
            )),
        }
    }
}

pub(super) fn capture_conpty_host_exits(pids: &[u32]) -> Result<Vec<ProcessExitWitness>, String> {
    let witnesses = pids
        .iter()
        .map(|&pid| {
            let handle = OwnedHandle::new(
                unsafe {
                    OpenProcess(
                        PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                        0,
                        pid,
                    )
                },
                &format!("capture ConPTY host {pid} process handle"),
            )?;
            Ok(ProcessExitWitness {
                pid,
                role: "ConPTY host",
                handle,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let attributable = conpty_host_children_of_current_process()?;
    for witness in &witnesses {
        if !attributable.contains(&witness.pid) {
            return Err(format!(
                "captured ConPTY host {} is not attributable to the current test process",
                witness.pid
            ));
        }
    }
    Ok(witnesses)
}

pub(super) fn wait_for_processes_exit(
    witnesses: Vec<ProcessExitWitness>,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    for witness in witnesses {
        witness.wait_until(deadline)?;
    }
    Ok(())
}

fn run_process_query(script: &str) -> Result<HashSet<u32>, String> {
    let output = run_powershell(script)?;
    if !output.status.success() {
        return Err(format!(
            "PowerShell process query failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .collect())
}

// cmd.output() waits for the pipe to close, and CreateProcess can leak our
// write-end into a concurrent ConPTY child, so wait and drain are both bounded.
fn run_powershell(script: &str) -> Result<std::process::Output, String> {
    let mut cmd = std::process::Command::new("powershell.exe");
    cmd.args([
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-Command",
        script,
    ]);
    codeterm_util::process_util::no_window_std(&mut cmd);
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("failed to start PowerShell process query: {error}"))?;

    let (out_tx, out_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let (err_tx, err_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| "PowerShell process query has no stdout pipe".to_string())?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| "PowerShell process query has no stderr pipe".to_string())?;
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        let _ = out_tx.send(buf);
    });
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        let _ = err_tx.send(buf);
    });

    let status = match child
        .wait_timeout(Duration::from_secs(10))
        .map_err(|error| format!("failed waiting for PowerShell process query: {error}"))?
    {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err("PowerShell process query exceeded its failure deadline".to_string());
        }
    };

    // A leaked pipe handle must become an observation failure, never an empty
    // result that could satisfy an absence assertion.
    let deadline = Instant::now() + Duration::from_millis(500);
    let stdout = recv_pipe_until(&out_rx, deadline, "stdout")?;
    let stderr = recv_pipe_until(&err_rx, deadline, "stderr")?;
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

fn recv_pipe_until(
    rx: &std::sync::mpsc::Receiver<Vec<u8>>,
    deadline: Instant,
    stream: &str,
) -> Result<Vec<u8>, String> {
    let budget = deadline.saturating_duration_since(Instant::now());
    rx.recv_timeout(budget)
        .map_err(|error| format!("PowerShell process query {stream} drain failed: {error}"))
}

fn direct_console_children(parent_pid: u32) -> Result<HashSet<u32>, String> {
    let snapshot = OwnedHandle::new(
        unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) },
        "failed to enumerate processes",
    )?;
    let mut entry = PROCESSENTRY32W::default();
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    if unsafe { Process32FirstW(snapshot.0, &mut entry) } == 0 {
        return Err(last_error("failed to read process snapshot"));
    }

    let mut matches = HashSet::new();
    loop {
        let name = wide_name(&entry.szExeFile);
        if entry.th32ParentProcessID == parent_pid
            && (name.eq_ignore_ascii_case("OpenConsole.exe")
                || name.eq_ignore_ascii_case("conhost.exe"))
        {
            matches.insert(entry.th32ProcessID);
        }
        if unsafe { Process32NextW(snapshot.0, &mut entry) } == 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                break;
            }
            return Err(format!("failed to advance process snapshot: {error}"));
        }
    }
    Ok(matches)
}

struct OwnedHandle(HANDLE);

impl OwnedHandle {
    fn new(handle: HANDLE, operation: &str) -> Result<Self, String> {
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            Err(last_error(operation))
        } else {
            Ok(Self(handle))
        }
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn wide_name(value: &[u16]) -> String {
    let end = value.iter().position(|ch| *ch == 0).unwrap_or(value.len());
    String::from_utf16_lossy(&value[..end])
}

fn last_error(operation: &str) -> String {
    format!("{operation}: {}", std::io::Error::last_os_error())
}
