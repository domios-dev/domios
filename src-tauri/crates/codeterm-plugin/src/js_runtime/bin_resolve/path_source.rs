//! Where the resolver reads the user's PATH as it stands now. A running process
//! never sees PATH edits an installer writes to the registry or a shell profile.

use std::ffi::OsString;

use codeterm_util::windows_env::read_persisted_path;
use parking_lot::RwLock;

use super::{BinSearchEnvironment, EnvironmentValue};

pub trait PathSource: Send + Sync {
    fn environment(&self) -> BinSearchEnvironment;
}

/// Process PATH, plus the persisted user and machine PATH: the registry on
/// Windows, the last login-shell snapshot elsewhere.
pub struct SystemPathSource;

impl PathSource for SystemPathSource {
    fn environment(&self) -> BinSearchEnvironment {
        let persisted = read_persisted_path();
        BinSearchEnvironment {
            inherited_path: std::env::var_os("PATH"),
            machine_path: persisted.machine,
            user_path: match persisted.user {
                EnvironmentValue::Missing => login_shell_path(),
                user => user,
            },
            pathext: std::env::var_os("PATHEXT"),
        }
    }
}

static LOGIN_SHELL_PATH: RwLock<Option<OsString>> = RwLock::new(None);

fn login_shell_path() -> EnvironmentValue {
    LOGIN_SHELL_PATH
        .read()
        .clone()
        .map_or(EnvironmentValue::Missing, EnvironmentValue::Value)
}

/// Re-read the PATH a new login shell would get. Blocking; Windows reads the
/// registry on every probe, so there it has nothing to do.
pub fn refresh_user_path() {
    #[cfg(unix)]
    match unix::read_login_shell_path() {
        Some(path) => *LOGIN_SHELL_PATH.write() = Some(path),
        None => log::debug!("[bin_resolve] login shell PATH unavailable; keeping last snapshot"),
    }
}

#[cfg(any(unix, test))]
const PATH_BEGIN: &[u8] = b"__DOMIOS_PATH_BEGIN__";
#[cfg(any(unix, test))]
const PATH_END: &[u8] = b"__DOMIOS_PATH_END__";

/// Interactive rc files may print around the marked PATH; only the markers count.
#[cfg(any(unix, test))]
fn parse_marked_path(output: &[u8]) -> Option<Vec<u8>> {
    let start = output
        .windows(PATH_BEGIN.len())
        .rposition(|window| window == PATH_BEGIN)?
        + PATH_BEGIN.len();
    let rest = &output[start..];
    let end = rest
        .windows(PATH_END.len())
        .position(|window| window == PATH_END)?;
    Some(rest[..end].to_vec()).filter(|path| !path.is_empty())
}

#[cfg(unix)]
mod unix {
    use std::ffi::OsString;
    use std::io::Read;
    use std::os::unix::ffi::OsStringExt;
    use std::process::{Command, Stdio};
    use std::time::Duration;

    const LOGIN_SHELL_DEADLINE: Duration = Duration::from_secs(10);

    pub(super) fn read_login_shell_path() -> Option<OsString> {
        let shell = std::env::var_os("SHELL")
            .filter(|shell| !shell.is_empty())
            .unwrap_or_else(|| "/bin/sh".into());
        let script = format!(
            "printf '%s%s%s' '{}' \"$PATH\" '{}'",
            String::from_utf8_lossy(super::PATH_BEGIN),
            String::from_utf8_lossy(super::PATH_END)
        );
        let mut child = Command::new(shell)
            .args(["-l", "-i", "-c", &script])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        let mut stdout = child.stdout.take()?;
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut output = Vec::new();
            let _ = stdout.read_to_end(&mut output);
            let _ = sender.send(output);
        });
        let output = receiver.recv_timeout(LOGIN_SHELL_DEADLINE).ok();
        if output.is_none() {
            let _ = child.kill();
        }
        let _ = child.wait();
        super::parse_marked_path(&output?).map(OsString::from_vec)
    }
}

#[cfg(test)]
mod tests;
