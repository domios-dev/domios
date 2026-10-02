use std::path::PathBuf;
#[cfg(any(windows, test))]
use std::path::Path;
use std::process::{Command, Stdio};

pub(super) struct UpdateCommandFailure {
    pub(super) reason: String,
    pub(super) child_stopped: bool,
}

pub(super) fn run_update_command(command: &str) -> Result<(), UpdateCommandFailure> {
    use wait_timeout::ChildExt;
    let (executable, args) = update_argv(command).map_err(|reason| UpdateCommandFailure {
        reason,
        child_stopped: true,
    })?;
    let mut process = Command::new(executable);
    process.args(args);
    codeterm_util::process_util::no_window_std(&mut process);
    let mut child = process
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| UpdateCommandFailure {
            reason: error.to_string(),
            child_stopped: true,
        })?;
    let wait_result = match child.wait_timeout(std::time::Duration::from_secs(300)) {
        Ok(result) => result,
        Err(error) => {
            let killed = child.kill().is_ok();
            let stopped = if killed {
                child.wait().is_ok()
            } else {
                child.try_wait().ok().flatten().is_some()
            };
            return Err(UpdateCommandFailure {
                reason: format!("waiting for provider update failed: {error}"),
                child_stopped: stopped,
            });
        }
    };
    match wait_result {
        Some(status) if status.success() => Ok(()),
        Some(status) => Err(UpdateCommandFailure {
            reason: format!("provider update exited with {status}"),
            child_stopped: true,
        }),
        None => {
            let killed = child.kill().is_ok();
            let stopped = if killed {
                child.wait().is_ok()
            } else {
                child.try_wait().ok().flatten().is_some()
            };
            Err(UpdateCommandFailure {
                reason: "provider update exceeded the 300 second deadline".into(),
                child_stopped: stopped,
            })
        }
    }
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
        _ => return Err("Node.js was not found while resolving the Codex Windows shim".into()),
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

