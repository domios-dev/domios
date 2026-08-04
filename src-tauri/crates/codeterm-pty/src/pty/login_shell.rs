//! Resolve the shell a new PTY should run.
//!
//! The daemon inherits `$SHELL` from whatever launched it (launchd, a terminal,
//! a previous login), so a later `chsh` never reaches newly spawned tabs. The
//! account database is authoritative and is re-read on every spawn.

/// Account login shell (Unix) or the preferred PowerShell/COMSPEC (Windows).
pub fn resolve_login_shell() -> String {
    #[cfg(target_os = "windows")]
    {
        // Avoid WindowsApps pwsh.exe — Store-packaged PowerShell can answer terminal
        // queries while leaking prompt output outside ConPTY. Prefer real installs.
        let candidates = [
            r"C:\Program Files\PowerShell\7\pwsh.exe",
            r"C:\Program Files (x86)\PowerShell\7\pwsh.exe",
        ];
        for c in candidates.iter() {
            if std::path::Path::new(c).is_file() {
                return (*c).to_string();
            }
        }
        if let Some(path) = which_exe("pwsh.exe") {
            return path;
        }
        if let Some(path) = which_exe("powershell.exe") {
            return path;
        }
        std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
    }

    #[cfg(not(target_os = "windows"))]
    {
        account_shell()
            .or_else(|| std::env::var("SHELL").ok().filter(|s| is_executable(s)))
            .unwrap_or_else(|| "/bin/sh".to_string())
    }
}

/// Login shell recorded in the passwd database for the current uid.
/// `getpwuid_r` keeps this safe under concurrent spawns — the non-`_r` variant
/// hands back a shared static buffer.
#[cfg(not(target_os = "windows"))]
fn account_shell() -> Option<String> {
    let mut buf = vec![0 as libc::c_char; 2048];
    let mut entry: libc::passwd = unsafe { std::mem::zeroed() };
    let mut found: *mut libc::passwd = std::ptr::null_mut();
    let rc = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            &mut entry,
            buf.as_mut_ptr(),
            buf.len(),
            &mut found,
        )
    };
    if rc != 0 || found.is_null() || entry.pw_shell.is_null() {
        return None;
    }
    unsafe { std::ffi::CStr::from_ptr(entry.pw_shell) }
        .to_str()
        .ok()
        .map(str::to_owned)
        .filter(|shell| is_executable(shell))
}

#[cfg(not(target_os = "windows"))]
fn is_executable(path: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    !path.is_empty()
        && std::fs::metadata(path)
            .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(target_os = "windows")]
fn which_exe(name: &str) -> Option<String> {
    let path = std::env::var("PATH").ok()?;
    for dir in path.split(';') {
        if dir.is_empty() {
            continue;
        }
        if is_windowsapps_path(dir) {
            continue;
        }
        let full = std::path::Path::new(dir).join(name);
        if full.is_file() {
            return Some(full.to_string_lossy().into_owned());
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn is_windowsapps_path(path: &str) -> bool {
    path.to_ascii_lowercase().contains(r"\windowsapps")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_shell_is_executable() {
        let shell = resolve_login_shell();
        assert!(!shell.is_empty());
        #[cfg(not(target_os = "windows"))]
        assert!(is_executable(&shell), "{shell} is not executable");
    }

    /// The stale-`$SHELL` bug: the account database — not the daemon's inherited
    /// env — decides which shell a new tab runs.
    #[cfg(not(target_os = "windows"))]
    #[test]
    fn account_shell_wins_over_a_stale_shell_env() {
        let Some(account) = account_shell() else {
            return;
        };
        let stale = if account == "/bin/sh" {
            "/bin/echo"
        } else {
            "/bin/sh"
        };
        std::env::set_var("SHELL", stale);
        let resolved = resolve_login_shell();
        std::env::remove_var("SHELL");
        assert_eq!(resolved, account);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windowsapps_paths_are_rejected_for_default_shell_lookup() {
        assert!(is_windowsapps_path(
            r"C:\Program Files\WindowsApps\Microsoft.PowerShell_7.6.3.0_x64__8wekyb3d8bbwe"
        ));
        assert!(is_windowsapps_path(
            r"C:\Users\dev\AppData\Local\Microsoft\WindowsApps"
        ));
        assert!(!is_windowsapps_path(r"C:\Program Files\PowerShell\7"));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn non_executable_paths_are_rejected() {
        assert!(!is_executable(""));
        assert!(!is_executable("/etc/hosts"));
        assert!(!is_executable("/no/such/shell"));
    }
}
