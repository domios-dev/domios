use std::collections::HashSet;
use std::time::Duration;

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
    cmd.output().ok()
}
