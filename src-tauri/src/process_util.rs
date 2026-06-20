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
