#[cfg(any(windows, test))]
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[path = "process.rs"]
mod process;
pub(super) use process::owner_alive;

pub(super) struct UpdateCommandFailure {
    pub(super) reason: String,
    pub(super) child_stopped: bool,
}

impl From<String> for UpdateCommandFailure {
    fn from(reason: String) -> Self {
        Self {
            reason,
            child_stopped: true,
        }
    }
}

pub(super) fn run_update_command(command: &str) -> Result<(), UpdateCommandFailure> {
    use wait_timeout::ChildExt;
    let (executable, args) = update_argv(command)?;
    let mut process = Command::new(executable);
    process.args(args);
    codeterm_util::process_util::no_window_std(&mut process);
    let tree = process::ProcessTree::prepare(&mut process)?;
    let mut child = process
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let mut stdout = None;
    let mut stderr = None;
    let result = (|| -> Result<Option<std::process::ExitStatus>, String> {
        stdout = Some(capture(
            child.stdout.take().ok_or("stdout capture unavailable")?,
        )?);
        stderr = Some(capture(
            child.stderr.take().ok_or("stderr capture unavailable")?,
        )?);
        tree.attach(&child)?;
        child
            .wait_timeout(std::time::Duration::from_secs(300))
            .map_err(|error| error.to_string())
    })();
    let completed = matches!(&result, Ok(Some(_)));
    let outcome = match result {
        Ok(Some(status)) if status.success() => Ok(()),
        Ok(Some(status)) => Err(format!("provider update exited with {status}")),
        Ok(None) => Err("provider update exceeded the 300 second deadline".into()),
        Err(error) => Err(format!("waiting for provider update failed: {error}")),
    };
    tree.terminate(&mut child);
    let stopped = completed
        || child
            .wait_timeout(std::time::Duration::from_secs(5))
            .ok()
            .flatten()
            .is_some();
    let output = format!(
        "stdout:\n{}\nstderr:\n{}",
        captured(stdout),
        captured(stderr)
    );
    outcome.map_err(|error| UpdateCommandFailure {
        reason: format!(
            "{error}\n{output}{}",
            if stopped {
                ""
            } else {
                "\nupdater termination unconfirmed; further updates are withheld"
            }
        ),
        child_stopped: stopped,
    })
}

fn capture(
    mut stream: impl std::io::Read + Send + 'static,
) -> Result<std::sync::mpsc::Receiver<String>, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("provider-update-capture".into())
        .spawn(move || {
            const LIMIT: usize = 32 * 1024;
            let mut tail = std::collections::VecDeque::with_capacity(LIMIT);
            let mut chunk = [0u8; 4096];
            loop {
                match stream.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(size) => {
                        let excess = (tail.len() + size).saturating_sub(LIMIT);
                        tail.drain(..excess);
                        tail.extend(&chunk[..size]);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
            let bytes = tail.into_iter().collect::<Vec<_>>();
            let _ = tx.send(String::from_utf8_lossy(&bytes).into_owned());
        })
        .map_err(|error| error.to_string())?;
    Ok(rx)
}

fn captured(rx: Option<std::sync::mpsc::Receiver<String>>) -> String {
    rx.and_then(|rx| rx.recv_timeout(std::time::Duration::from_secs(5)).ok())
        .unwrap_or_else(|| "capture stream did not close before its failure deadline".into())
}

fn update_argv(command: &str) -> Result<(PathBuf, Vec<std::ffi::OsString>), String> {
    let mut parts = command.split_whitespace();
    let executable = parts
        .next()
        .ok_or_else(|| "empty provider update command".to_string())?;
    if !executable
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "_./\\:-".contains(ch))
    {
        return Err("provider update executable contains unsupported characters".into());
    }
    let args = parts.collect::<Vec<_>>();
    if args.iter().any(|arg| {
        !arg.chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "_./\\:-".contains(ch))
    }) {
        return Err("provider update arguments contain unsupported characters".into());
    }
    let resolved = match codeterm_plugin::js_runtime::probe_bin(executable).resolution {
        codeterm_plugin::js_runtime::BinResolution::Found(path) => path,
        codeterm_plugin::js_runtime::BinResolution::Missing
        | codeterm_plugin::js_runtime::BinResolution::Unknown => {
            #[cfg(windows)]
            return Err(format!(
                "provider update executable {executable} was not resolved through PATH/PATHEXT"
            ));
            #[cfg(not(windows))]
            PathBuf::from(executable)
        }
    };
    #[cfg(windows)]
    if resolved
        .extension()
        .is_some_and(|extension| extension.to_string_lossy().eq_ignore_ascii_case("cmd"))
    {
        return windows_npm_shim_argv(&resolved, &args);
    }
    Ok((resolved, args.into_iter().map(Into::into).collect()))
}

#[cfg(windows)]
fn windows_npm_shim_argv(
    shim_path: &Path,
    args: &[&str],
) -> Result<(PathBuf, Vec<std::ffi::OsString>), String> {
    let contents = std::fs::read_to_string(shim_path).map_err(|error| error.to_string())?;
    let script = npm_shim_script(shim_path.parent().unwrap_or(Path::new(".")), &contents)
        .ok_or_else(|| {
            format!(
                "could not resolve the node entrypoint in {}",
                shim_path.display()
            )
        })?;
    let node = match codeterm_plugin::js_runtime::probe_bin("node").resolution {
        codeterm_plugin::js_runtime::BinResolution::Found(path) => path,
        _ => return Err("Node.js was not found while resolving the provider Windows shim".into()),
    };
    let mut argv = vec![script.into_os_string()];
    argv.extend(args.iter().map(std::ffi::OsString::from));
    Ok((node, argv))
}

#[cfg(any(windows, test))]
pub(super) fn npm_shim_script(parent: &Path, contents: &str) -> Option<PathBuf> {
    let marker = "\"%_prog%\" \"";
    let line = contents.lines().find(|line| {
        line.to_ascii_lowercase()
            .contains(&marker.to_ascii_lowercase())
            && line.contains("%*")
    })?;
    let start = line
        .to_ascii_lowercase()
        .find(&marker.to_ascii_lowercase())?
        + marker.len();
    let target = line[start..].split_once('"')?.0;
    let relative = target
        .strip_prefix("%dp0%")?
        .trim_start_matches(['\\', '/']);
    let relative = relative.replace('\\', "/");
    Some(parent.join(relative))
}

pub(super) fn platform_key() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "macos",
        _ => "linux",
    }
}

#[cfg(test)]
#[path = "command_tests.rs"]
mod tests;
