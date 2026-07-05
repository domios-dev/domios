//! Cross-cutting `Command`-spawning helpers. The daemon is a windowed Tauri app,
//! so a console subprocess flashes a `conhost.exe` window on Windows by default;
//! route every spawn through here to suppress the flicker.

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Suppress the auto-spawned console window for `std::process::Command`
/// on Windows. No-op everywhere else.
#[cfg(windows)]
pub fn no_window_std(cmd: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
#[inline]
pub fn no_window_std(_cmd: &mut std::process::Command) {}

/// Same idea for `tokio::process::Command`. tokio exposes
/// `creation_flags` as an inherent method on Windows, so no trait
/// import is needed here.
#[cfg(windows)]
pub fn no_window_tokio(cmd: &mut tokio::process::Command) {
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
#[inline]
pub fn no_window_tokio(_cmd: &mut tokio::process::Command) {}

/// Spawn a subprocess that disclaims TCC responsibility, capturing its stdout.
/// macOS "App Data" protection silently EPERMs a signed bundle (and its children)
/// reading another app's Group Containers; disclaiming makes the child its own
/// responsible process (CLI-tool authority), so it can read those dirs.
#[cfg(target_os = "macos")]
pub fn disclaimed_capture(program: &str, args: &[&str]) -> Option<String> {
    use std::ffi::CString;
    use std::io::Read;
    use std::os::unix::io::FromRawFd;

    extern "C" {
        fn responsibility_spawnattrs_setdisclaim(
            attrs: *mut libc::posix_spawnattr_t,
            disclaim: libc::c_int,
        ) -> libc::c_int;
        static environ: *const *const libc::c_char;
    }

    let c_program = CString::new(program).ok()?;
    let c_args: Vec<CString> = std::iter::once(program)
        .chain(args.iter().copied())
        .filter_map(|s| CString::new(s).ok())
        .collect();
    let mut argv: Vec<*mut libc::c_char> = c_args.iter().map(|s| s.as_ptr() as *mut _).collect();
    argv.push(std::ptr::null_mut());

    unsafe {
        let mut fds = [0i32; 2];
        if libc::pipe(fds.as_mut_ptr()) != 0 {
            return None;
        }
        let (read_fd, write_fd) = (fds[0], fds[1]);

        let mut actions: libc::posix_spawn_file_actions_t = std::mem::zeroed();
        libc::posix_spawn_file_actions_init(&mut actions);
        libc::posix_spawn_file_actions_adddup2(&mut actions, write_fd, libc::STDOUT_FILENO);
        libc::posix_spawn_file_actions_addclose(&mut actions, read_fd);
        libc::posix_spawn_file_actions_addclose(&mut actions, write_fd);

        let mut attr: libc::posix_spawnattr_t = std::mem::zeroed();
        libc::posix_spawnattr_init(&mut attr);
        responsibility_spawnattrs_setdisclaim(&mut attr, 1);

        let mut pid: libc::pid_t = 0;
        let rc = libc::posix_spawn(
            &mut pid,
            c_program.as_ptr(),
            &actions,
            &attr,
            argv.as_ptr(),
            environ as *const *mut libc::c_char,
        );
        libc::posix_spawn_file_actions_destroy(&mut actions);
        libc::posix_spawnattr_destroy(&mut attr);
        libc::close(write_fd);

        if rc != 0 {
            libc::close(read_fd);
            return None;
        }

        let mut file = std::fs::File::from_raw_fd(read_fd);
        let mut out = String::new();
        let _ = file.read_to_string(&mut out);
        drop(file);
        let mut status = 0i32;
        libc::waitpid(pid, &mut status, 0);
        Some(out)
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn disclaimed_capture_runs_and_captures_stdout() {
        let out = disclaimed_capture("/bin/sh", &["-c", "printf '{\"ok\":1}'"]);
        assert_eq!(out.as_deref(), Some("{\"ok\":1}"));
    }

    #[test]
    fn disclaimed_capture_absent_program_is_none() {
        assert!(disclaimed_capture("/nonexistent/binary", &[]).is_none());
    }
}

#[cfg(test)]
mod spawn_audit_tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    #[test]
    fn daemon_windows_spawns_use_no_window_helper() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut failures = Vec::new();
        audit_dir(&root, &root, &mut failures);
        assert!(
            failures.is_empty(),
            "Windows daemon subprocesses must call process_util::no_window_* unless they are visible UI openers or non-Windows-only.\n{}",
            failures.join("\n")
        );
    }

    fn audit_dir(root: &Path, dir: &Path, failures: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if !skip_dir(root, &path) {
                    audit_dir(root, &path, failures);
                }
                continue;
            }
            if path.extension().and_then(|s| s.to_str()) == Some("rs") && !skip_file(root, &path) {
                audit_file(root, &path, failures);
            }
        }
    }

    fn audit_file(root: &Path, path: &Path, failures: &mut Vec<String>) {
        let Ok(source) = fs::read_to_string(path) else {
            return;
        };
        let lines: Vec<&str> = source.lines().collect();
        for (idx, line) in lines.iter().enumerate() {
            if !line.contains("Command::new(") && !line.contains("std::process::Command::new(") {
                continue;
            }
            let before = window(&lines, idx.saturating_sub(8), idx + 1).join("\n");
            let after = window(&lines, idx, idx + 40).join("\n");
            if after.contains("process_util::no_window_std")
                || after.contains("process_util::no_window_tokio")
                || before.contains("target_os = \"macos\"")
                || before.contains("target_os = \"linux\"")
                || before.contains("not(windows)")
                || allowed_visible_or_unix_spawn(line)
            {
                continue;
            }
            failures.push(format!(
                "{}:{}: {}",
                path.strip_prefix(root)
                    .unwrap_or(path)
                    .display()
                    .to_string()
                    .replace('\\', "/"),
                idx + 1,
                line.trim()
            ));
        }
    }

    fn window<'a>(lines: &'a [&str], start: usize, end: usize) -> &'a [&'a str] {
        &lines[start.min(lines.len())..end.min(lines.len())]
    }

    fn skip_dir(root: &Path, path: &Path) -> bool {
        let rel = path.strip_prefix(root).unwrap_or(path).to_string_lossy();
        matches!(rel.as_ref(), "bin" | "pty")
    }

    fn skip_file(root: &Path, path: &Path) -> bool {
        let rel = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        rel == "process_util.rs" || rel.ends_with("/tests.rs") || rel == "keepawake/linux.rs"
    }

    fn allowed_visible_or_unix_spawn(line: &str) -> bool {
        [
            "\"explorer\"",
            "\"open\"",
            "\"xdg-open\"",
            "\"osascript\"",
            "\"xclip\"",
            "\"ps\"",
            "\"kill\"",
            "\"/bin/sh\"",
            "\"launchctl\"",
            "\"systemctl\"",
            "\"/usr/bin/id\"",
        ]
        .iter()
        .any(|needle| line.contains(needle))
    }
}
