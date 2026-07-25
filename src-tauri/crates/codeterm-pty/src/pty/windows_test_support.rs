use std::collections::HashSet;
use std::io::Read;
use std::process::Stdio;
use std::time::Duration;

use wait_timeout::ChildExt;

// Hosts are attributed by parent PID, which is the whole test process, so a
// concurrent test's ConPTY would miscount as this one's. Hold it while observing.
pub(super) fn exclusive_conpty_test() -> parking_lot::MutexGuard<'static, ()> {
    static LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
    LOCK.lock()
}

pub(super) fn legacy_conhost_children(parent_pid: u32) -> Vec<u32> {
    let script = format!(
        "$p={}; Get-CimInstance Win32_Process -Filter \"Name = 'conhost.exe' AND ParentProcessId = $p\" | Where-Object {{ $_.CommandLine -like '*conhost.exe 0x4*' }} | ForEach-Object {{ $_.ProcessId }}",
        parent_pid
    );
    run_process_query(&script).into_iter().collect()
}

pub(super) fn conpty_host_children_of_current_process() -> HashSet<u32> {
    let script = format!(
        "$p={}; Get-CimInstance Win32_Process -Filter \"ParentProcessId = $p AND (Name = 'OpenConsole.exe' OR Name = 'conhost.exe')\" | Where-Object {{ $_.Name -eq 'OpenConsole.exe' -or $_.CommandLine -like '*--headless*' }} | ForEach-Object {{ $_.ProcessId }}",
        std::process::id()
    );
    run_process_query(&script)
}

pub(super) fn new_conpty_hosts(before: &HashSet<u32>) -> Vec<u32> {
    conpty_host_children_of_current_process()
        .difference(before)
        .copied()
        .collect()
}

pub(super) fn wait_for_processes_gone(pids: Vec<u32>, timeout: Duration) -> bool {
    if pids.is_empty() {
        return true;
    }
    let ids = pids
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let timeout_secs = timeout.as_secs().max(1);
    let script = format!(
        "$ids=@({ids}); Wait-Process -Id $ids -Timeout {timeout_secs} -ErrorAction SilentlyContinue; if (Get-Process -Id $ids -ErrorAction SilentlyContinue) {{ exit 1 }} else {{ exit 0 }}"
    );
    run_powershell(&script).is_some_and(|output| output.status.success())
}

fn run_process_query(script: &str) -> HashSet<u32> {
    let Some(output) = run_powershell(script) else {
        return HashSet::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse::<u32>().ok())
        .collect()
}

// cmd.output() waits for the pipe to close, and CreateProcess can leak our
// write-end into a concurrent ConPTY child, so wait and drain are both bounded.
fn run_powershell(script: &str) -> Option<std::process::Output> {
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
        .ok()?;

    let (out_tx, out_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let (err_tx, err_rx) = std::sync::mpsc::channel::<Vec<u8>>();
    let mut stdout = child.stdout.take()?;
    let mut stderr = child.stderr.take()?;
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

    let status = match child.wait_timeout(Duration::from_secs(10)).ok()? {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
    };

    // Short shared drain deadline: a leaked handle means the pipe may never
    // close on its own, so this returns whatever arrived rather than blocking.
    let deadline = std::time::Instant::now() + Duration::from_millis(500);
    let recv_until = |rx: &std::sync::mpsc::Receiver<Vec<u8>>| {
        let budget = deadline.saturating_duration_since(std::time::Instant::now());
        rx.recv_timeout(budget).unwrap_or_default()
    };
    Some(std::process::Output {
        status,
        stdout: recv_until(&out_rx),
        stderr: recv_until(&err_rx),
    })
}
