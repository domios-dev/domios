#[cfg(any(windows, test))]
use std::path::{Path, PathBuf};

#[cfg(any(windows, test))]
#[derive(Debug, PartialEq, Eq)]
struct ProcessCandidate {
    pid: u32,
    parent_pid: u32,
    parent_alive: bool,
    executable_path: PathBuf,
}

#[cfg(any(windows, test))]
#[derive(Clone, Copy)]
enum HostSelection {
    /// Hosts whose parent is absent from the same snapshot. A sibling instance
    /// running this same install keeps its hosts; only leftovers are selected.
    Orphaned,
    /// Every host running the exact bundled image, ours included. The file
    /// cannot be replaced while any process runs it, so replacement needs this.
    All,
}

#[cfg(any(windows, test))]
fn is_selected_bundled_host(
    candidate: &ProcessCandidate,
    _owner_pid: u32,
    expected_executable: &Path,
    selection: HostSelection,
) -> bool {
    windows_paths_match(&candidate.executable_path, expected_executable)
        && match selection {
            HostSelection::Orphaned => !candidate.parent_alive,
            HostSelection::All => true,
        }
}

#[cfg(any(windows, test))]
fn windows_paths_match(left: &Path, right: &Path) -> bool {
    normalized_windows_path(left).eq_ignore_ascii_case(&normalized_windows_path(right))
}

#[cfg(any(windows, test))]
fn normalized_windows_path(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('/', "\\");
    normalized
        .strip_prefix(r"\\?\")
        .unwrap_or(&normalized)
        .to_string()
}

#[cfg(windows)]
pub(super) async fn cleanup_stale_before_shutdown() -> Result<usize, String> {
    tokio::task::spawn_blocking(cleanup_stale_from_environment)
        .await
        .map_err(|error| format!("stale bundled ConPTY cleanup task failed: {error}"))?
}

#[cfg(not(windows))]
pub(super) async fn cleanup_stale_before_shutdown() -> Result<usize, String> {
    Ok(0)
}

#[cfg(windows)]
pub(super) async fn cleanup_before_installer() -> Result<usize, String> {
    tokio::task::spawn_blocking(cleanup_from_environment)
        .await
        .map_err(|error| format!("bundled ConPTY cleanup task failed: {error}"))?
}

#[cfg(not(windows))]
pub(super) async fn cleanup_before_installer() -> Result<usize, String> {
    Ok(0)
}

#[cfg(windows)]
fn cleanup_stale_from_environment() -> Result<usize, String> {
    let Some(dir) = std::env::var_os("CODETERM_CONPTY_DIR") else {
        return Ok(0);
    };
    cleanup_bundled_hosts(Path::new(&dir), std::process::id(), HostSelection::Orphaned)
}

#[cfg(windows)]
fn cleanup_from_environment() -> Result<usize, String> {
    let Some(dir) = std::env::var_os("CODETERM_CONPTY_DIR") else {
        return Ok(0);
    };
    cleanup_bundled_hosts(Path::new(&dir), std::process::id(), HostSelection::All)
}

/// Reap bundled hosts left behind by a previous run. Runs off the startup path;
/// hosts owned by any live process (this one or another instance) are spared.
///
/// `Orphaned` is load-bearing here: this races our own first PTY spawns, and a
/// host born now has us as a live parent, so a wider selection would kill it.
#[cfg(windows)]
pub(crate) fn reap_orphaned_hosts_in_background() {
    std::thread::spawn(|| {
        let Some(dir) = std::env::var_os("CODETERM_CONPTY_DIR") else {
            return;
        };
        match cleanup_bundled_hosts(Path::new(&dir), std::process::id(), HostSelection::Orphaned) {
            Ok(0) => {}
            Ok(count) => log::info!("[conpty] reaped {count} orphaned bundled ConPTY host(s)"),
            Err(error) => log::warn!("[conpty] orphan sweep failed: {error}"),
        }
    });
}

#[cfg(windows)]
fn cleanup_bundled_hosts(
    dir: &Path,
    owner_pid: u32,
    selection: HostSelection,
) -> Result<usize, String> {
    use std::time::{Duration, Instant};

    let expected = dir.join("OpenConsole.exe");
    if !expected.exists() {
        return Ok(0);
    }
    let expected = std::fs::canonicalize(&expected)
        .map_err(|error| format!("failed to resolve {}: {error}", expected.display()))?;
    let processes = windows::matching_processes(owner_pid, &expected, selection)?;
    for process in &processes {
        process.terminate()?;
    }
    let deadline = Instant::now() + Duration::from_secs(10);
    for process in &processes {
        process.wait_until(deadline)?;
    }
    if matches!(selection, HostSelection::All) {
        windows::ensure_file_replaceable(&expected)?;
    }
    Ok(processes.len())
}

#[cfg(windows)]
mod windows {
    use super::{is_selected_bundled_host, HostSelection, ProcessCandidate};
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use std::time::Instant;
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_NO_MORE_FILES, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE, WAIT_FAILED,
        WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, DELETE, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_DELETE, FILE_SHARE_READ,
        FILE_SHARE_WRITE, OPEN_EXISTING, SYNCHRONIZE,
    };
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, TerminateProcess, WaitForSingleObject,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE,
    };

    pub(super) struct ProcessHandle {
        pid: u32,
        handle: OwnedHandle,
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

    impl ProcessHandle {
        pub(super) fn terminate(&self) -> Result<(), String> {
            if unsafe { TerminateProcess(self.handle.0, 1) } != 0 {
                return Ok(());
            }
            if unsafe { WaitForSingleObject(self.handle.0, 0) } == WAIT_OBJECT_0 {
                return Ok(());
            }
            Err(last_error(&format!(
                "failed to terminate bundled OpenConsole process {}",
                self.pid
            )))
        }

        pub(super) fn wait_until(&self, deadline: Instant) -> Result<(), String> {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| format!("bundled OpenConsole process {} did not exit", self.pid))?;
            let millis = remaining.as_millis().clamp(1, u32::MAX as u128) as u32;
            match unsafe { WaitForSingleObject(self.handle.0, millis) } {
                WAIT_OBJECT_0 => Ok(()),
                WAIT_TIMEOUT => Err(format!(
                    "bundled OpenConsole process {} did not exit before deadline",
                    self.pid
                )),
                WAIT_FAILED => Err(last_error(&format!(
                    "failed waiting for bundled OpenConsole process {}",
                    self.pid
                ))),
                result => Err(format!(
                    "unexpected wait result {result} for bundled OpenConsole process {}",
                    self.pid
                )),
            }
        }
    }

    pub(super) fn matching_processes(
        owner_pid: u32,
        expected: &Path,
        selection: HostSelection,
    ) -> Result<Vec<ProcessHandle>, String> {
        let snapshot = OwnedHandle::new(
            unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) },
            "failed to enumerate processes",
        )?;
        let mut entry = PROCESSENTRY32W::default();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if unsafe { Process32FirstW(snapshot.0, &mut entry) } == 0 {
            return Err(last_error("failed to read process snapshot"));
        }

        // Parent liveness is judged against the same snapshot that produced the
        // hosts, so a parent exiting mid-sweep cannot flip a host's classification.
        let mut live_pids = std::collections::HashSet::new();
        let mut hosts = Vec::new();
        loop {
            live_pids.insert(entry.th32ProcessID);
            if wide_name(&entry.szExeFile).eq_ignore_ascii_case("OpenConsole.exe") {
                hosts.push((entry.th32ProcessID, entry.th32ParentProcessID));
            }
            if unsafe { Process32NextW(snapshot.0, &mut entry) } == 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(ERROR_NO_MORE_FILES as i32) {
                    break;
                }
                return Err(format!("failed to advance process snapshot: {error}"));
            }
        }

        let mut matches = Vec::new();
        for (pid, parent_pid) in hosts {
            let parent_alive = live_pids.contains(&parent_pid);
            if let Some(process) = open_matching_process(
                pid,
                parent_pid,
                parent_alive,
                owner_pid,
                expected,
                selection,
            ) {
                matches.push(process);
            }
        }
        Ok(matches)
    }

    /// A host this process cannot open or query is one it could never terminate
    /// either, so it is skipped rather than aborting the sweep for every host.
    fn open_matching_process(
        pid: u32,
        parent_pid: u32,
        parent_alive: bool,
        owner_pid: u32,
        expected: &Path,
        selection: HostSelection,
    ) -> Option<ProcessHandle> {
        let query_handle = OwnedHandle::new(
            unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) },
            "query",
        )
        .inspect_err(|error| log::debug!("[conpty] skipping OpenConsole {pid}: {error}"))
        .ok()?;
        let executable_path = query_executable_path(query_handle.0, pid)
            .inspect_err(|error| log::debug!("[conpty] skipping OpenConsole {pid}: {error}"))
            .ok()?;
        let candidate = ProcessCandidate {
            pid,
            parent_pid,
            parent_alive,
            executable_path,
        };
        if !is_selected_bundled_host(&candidate, owner_pid, expected, selection) {
            return None;
        }
        let access = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | SYNCHRONIZE;
        let handle = OwnedHandle::new(
            unsafe { OpenProcess(access, 0, candidate.pid) },
            "terminate",
        )
        .inspect_err(|error| log::debug!("[conpty] skipping OpenConsole {pid}: {error}"))
        .ok()?;
        let executable_path = query_executable_path(handle.0, candidate.pid).ok()?;
        let confirmed = ProcessCandidate {
            executable_path,
            ..candidate
        };
        is_selected_bundled_host(&confirmed, owner_pid, expected, selection).then_some(
            ProcessHandle {
                pid: confirmed.pid,
                handle,
            },
        )
    }

    fn query_executable_path(handle: HANDLE, pid: u32) -> Result<PathBuf, String> {
        let mut path = vec![0u16; 32_768];
        let mut len = path.len() as u32;
        if unsafe { QueryFullProcessImageNameW(handle, 0, path.as_mut_ptr(), &mut len) } == 0 {
            return Err(last_error(&format!(
                "failed to query OpenConsole process {pid} path"
            )));
        }
        path.truncate(len as usize);
        Ok(PathBuf::from(String::from_utf16_lossy(&path)))
    }

    pub(super) fn ensure_file_replaceable(path: &Path) -> Result<(), String> {
        let wide = OsStr::new(path)
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let access = DELETE | GENERIC_WRITE;
        let sharing = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
        OwnedHandle::new(
            unsafe {
                CreateFileW(
                    wide.as_ptr(),
                    access,
                    sharing,
                    std::ptr::null(),
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    std::ptr::null_mut(),
                )
            },
            &format!("{} is still locked against replacement", path.display()),
        )?;
        Ok(())
    }

    fn wide_name(buffer: &[u16]) -> String {
        let len = buffer
            .iter()
            .position(|value| *value == 0)
            .unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..len])
    }

    fn last_error(operation: &str) -> String {
        format!("{operation}: {}", std::io::Error::last_os_error())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(pid: u32, parent_pid: u32, path: &str) -> ProcessCandidate {
        ProcessCandidate {
            pid,
            parent_pid,
            parent_alive: true,
            executable_path: PathBuf::from(path),
        }
    }

    fn orphan(pid: u32, parent_pid: u32, path: &str) -> ProcessCandidate {
        ProcessCandidate {
            parent_alive: false,
            ..candidate(pid, parent_pid, path)
        }
    }

    #[test]
    fn orphan_selection_spares_every_host_with_a_live_parent() {
        let expected = Path::new(r"C:\CodeTerm\conpty\x64\OpenConsole.exe");
        let path = expected.to_str().unwrap();

        assert!(is_selected_bundled_host(
            &orphan(10, 8, path),
            7,
            expected,
            HostSelection::Orphaned,
        ));

        // A sibling instance of this same install owns hosts with a foreign but
        // LIVE parent. Its terminals are not ours to reap.
        let other_instance = candidate(10, 8, path);
        assert!(!is_selected_bundled_host(
            &other_instance,
            7,
            expected,
            HostSelection::Orphaned,
        ));
        assert!(is_selected_bundled_host(
            &other_instance,
            7,
            expected,
            HostSelection::All,
        ));

        assert!(!is_selected_bundled_host(
            &candidate(10, 7, path),
            7,
            expected,
            HostSelection::Orphaned,
        ));
    }

    #[test]
    fn orphan_selection_still_requires_the_exact_bundled_path() {
        let expected = Path::new(r"C:\CodeTerm\conpty\x64\OpenConsole.exe");
        for path in [
            r"C:\Windows\System32\OpenConsole.exe",
            r"C:\CodeTerm-dev\conpty\x64\OpenConsole.exe",
        ] {
            assert!(!is_selected_bundled_host(
                &orphan(10, 8, path),
                7,
                expected,
                HostSelection::Orphaned,
            ));
        }
    }

    #[test]
    fn orphan_selection_matches_the_bundled_path_case_insensitively() {
        let expected =
            Path::new(r"C:\Users\dev\AppData\Local\CodeTerm\conpty\x64\OpenConsole.exe");
        assert!(is_selected_bundled_host(
            &orphan(
                10,
                8,
                r"\\?\c:\users\user\appdata\local\codeterm\conpty\x64\OPENCONSOLE.EXE"
            ),
            7,
            expected,
            HostSelection::Orphaned,
        ));
        assert!(!is_selected_bundled_host(
            &orphan(10, 8, r"C:\work\codeterm\conpty\x64\OpenConsole.exe"),
            7,
            expected,
            HostSelection::Orphaned,
        ));
    }

    #[test]
    fn final_selection_accepts_exact_host_from_any_parent() {
        let expected = Path::new(r"C:\CodeTerm\conpty\x64\OpenConsole.exe");
        for parent_pid in [7, 8] {
            assert!(is_selected_bundled_host(
                &candidate(10, parent_pid, expected.to_str().unwrap()),
                7,
                expected,
                HostSelection::All,
            ));
        }
    }

    #[test]
    fn selection_rejects_in_box_and_unrelated_hosts() {
        let expected = Path::new(r"C:\CodeTerm\conpty\x64\OpenConsole.exe");
        for path in [
            r"C:\Windows\System32\conhost.exe",
            r"C:\Windows\System32\OpenConsole.exe",
            r"C:\CodeTerm-dev\conpty\x64\OpenConsole.exe",
            r"C:\CodeTerm\conpty\arm64\OpenConsole.exe",
        ] {
            assert!(!is_selected_bundled_host(
                &candidate(10, 8, path),
                7,
                expected,
                HostSelection::All,
            ));
        }
    }

    #[cfg(windows)]
    #[test]
    fn cleanup_reaps_exact_openconsole_and_allows_replacement() {
        use std::process::{Command, Stdio};
        use std::time::Duration;

        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("conpty").join("x64");
        std::fs::create_dir_all(&dir).unwrap();
        let host = dir.join("OpenConsole.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &host).unwrap();

        let mut command = Command::new(&host);
        command
            .args([
                "--exact",
                "update::conpty_cleanup::tests::openconsole_test_helper",
                "--nocapture",
            ])
            .env("CODETERM_OPENCONSOLE_TEST_HELPER", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        crate::process_util::no_window_std(&mut command);
        let mut child = command.spawn().unwrap();

        let readiness = wait_for_helper_ready(&mut child, Duration::from_secs(10));
        if !readiness {
            let _ = child.kill();
            let _ = child.wait();
        }
        assert!(readiness, "OpenConsole helper did not become ready");
        assert!(
            windows::ensure_file_replaceable(&host).is_err(),
            "running OpenConsole helper must lock its executable"
        );
        let cleaned = cleanup_bundled_hosts(&dir, std::process::id(), HostSelection::All).unwrap();
        assert_eq!(cleaned, 1);
        child.wait().unwrap();

        let replacement = dir.join("replacement.exe");
        std::fs::write(&replacement, b"replacement").unwrap();
        std::fs::remove_file(&host).unwrap();
        std::fs::rename(&replacement, &host).unwrap();
        assert_eq!(std::fs::read(&host).unwrap(), b"replacement");
    }

    #[cfg(windows)]
    #[test]
    fn orphan_cleanup_spares_a_live_parents_host_but_all_reaps_it() {
        use std::process::{Command, Stdio};
        use std::time::Duration;

        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("conpty").join("x64");
        std::fs::create_dir_all(&dir).unwrap();
        let host = dir.join("OpenConsole.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &host).unwrap();

        let mut command = Command::new(&host);
        command
            .args([
                "--exact",
                "update::conpty_cleanup::tests::openconsole_test_helper",
                "--nocapture",
            ])
            .env("CODETERM_OPENCONSOLE_TEST_HELPER", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped());
        crate::process_util::no_window_std(&mut command);
        let mut child = command.spawn().unwrap();

        let readiness = wait_for_helper_ready(&mut child, Duration::from_secs(10));
        if !readiness {
            let _ = child.kill();
            let _ = child.wait();
        }
        assert!(readiness, "OpenConsole helper did not become ready");

        // The helper's parent is this live test process, so the pre-installer
        // sweep must leave it alone; only the replacement sweep may reap it.
        let spared = cleanup_bundled_hosts(&dir, u32::MAX, HostSelection::Orphaned).unwrap();
        assert_eq!(spared, 0);
        assert!(
            child.try_wait().unwrap().is_none(),
            "helper was reaped early"
        );

        let cleaned = cleanup_bundled_hosts(&dir, u32::MAX, HostSelection::All).unwrap();
        assert_eq!(cleaned, 1);
        child.wait().unwrap();
    }

    #[cfg(windows)]
    fn wait_for_helper_ready(
        child: &mut std::process::Child,
        timeout: std::time::Duration,
    ) -> bool {
        use std::io::BufRead;
        use std::sync::mpsc;

        let stdout = child.stdout.take();
        if stdout.is_none() {
            return false;
        }
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(stdout.unwrap());
            let mut line = String::new();
            loop {
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => {
                        let _ = tx.send(false);
                        break;
                    }
                    Ok(_) if line.contains("READY") => {
                        let _ = tx.send(true);
                        break;
                    }
                    Ok(_) => line.clear(),
                }
            }
        });
        rx.recv_timeout(timeout).unwrap_or(false)
    }

    #[cfg(windows)]
    #[test]
    fn openconsole_test_helper() {
        if std::env::var_os("CODETERM_OPENCONSOLE_TEST_HELPER").is_none() {
            return;
        }
        use std::io::{Read, Write};
        println!("READY");
        std::io::stdout().flush().unwrap();
        let _ = std::io::stdin().read(&mut [0u8; 1]);
    }
}
