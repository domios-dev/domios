use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use wait_timeout::ChildExt;
const TURN_CAPTURE_DEADLINE: Duration = Duration::from_secs(12);
const GIT_COMMAND_DEADLINE: Duration = Duration::from_secs(4);
const MAX_GIT_OUTPUT_BYTES: usize = 1024 * 1024;
#[derive(Debug)]
pub enum CaptureFailure {
    Deadline,
    Git(String),
}

pub fn delete_attempt_checkpoint_refs(cwd: &Path, attempt_id: &str) -> Result<(), String> {
    if !safe_ref_component(attempt_id) {
        return Ok(());
    }
    let deadline = Instant::now() + TURN_CAPTURE_DEADLINE;
    let prefix = format!("refs/domios/checkpoints/{attempt_id}");
    let refs = run_git(
        cwd,
        &["for-each-ref", "--format=%(refname)", &prefix],
        None,
        deadline,
    )
    .map_err(capture_failure_text)?;
    for reference in refs
        .lines()
        .filter(|reference| checkpoint_turn(&prefix, reference).is_some())
    {
        run_git(cwd, &["update-ref", "-d", reference], None, deadline)
            .map_err(capture_failure_text)?;
    }
    Ok(())
}

pub fn checkpoint_turn(prefix: &str, reference: &str) -> Option<u32> {
    reference
        .strip_prefix(prefix)?
        .strip_prefix('/')?
        .parse()
        .ok()
}

pub fn safe_ref_component(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub fn run_git(
    cwd: &Path,
    args: &[&str],
    index: Option<(&str, &std::ffi::OsStr)>,
    deadline: Instant,
) -> Result<String, CaptureFailure> {
    let output = run_git_status_with_index(cwd, args, index, deadline)?;
    if !output.status.success() {
        return Err(CaptureFailure::Git(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub fn run_git_status(
    cwd: &Path,
    args: &[&str],
    deadline: Instant,
) -> Result<Output, CaptureFailure> {
    run_git_status_with_index(cwd, args, None, deadline)
}

fn run_git_status_with_index(
    cwd: &Path,
    args: &[&str],
    index: Option<(&str, &std::ffi::OsStr)>,
    deadline: Instant,
) -> Result<Output, CaptureFailure> {
    let mut command = Command::new("git");
    codeterm_util::process_util::no_window_std(&mut command);
    // Optional locks off: these reads run beside the agent's own git and must not take index.lock.
    command
        .current_dir(cwd)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some((key, value)) = index {
        command.env(key, value);
    }
    let mut child = command
        .spawn()
        .map_err(|error| CaptureFailure::Git(error.to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| CaptureFailure::Git("git stdout pipe missing".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| CaptureFailure::Git("git stderr pipe missing".into()))?;
    let remaining = deadline
        .saturating_duration_since(Instant::now())
        .min(GIT_COMMAND_DEADLINE);
    let command_deadline = Instant::now() + remaining;
    let (sender, receiver) = std::sync::mpsc::channel();
    let stdout_sender = sender.clone();
    let stdout_reader = thread::spawn(move || {
        let _ = stdout_sender.send((true, read_bounded(stdout)));
    });
    let stderr_reader = thread::spawn(move || {
        let _ = sender.send((false, read_bounded(stderr)));
    });
    let waited = if remaining.is_zero() {
        None
    } else {
        match child.wait_timeout(remaining) {
            Ok(status) => status,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(CaptureFailure::Git(error.to_string()));
            }
        }
    };
    let Some(status) = waited else {
        let _ = child.kill();
        let _ = child.wait();
        return Err(CaptureFailure::Deadline);
    };
    let (mut stdout, mut stderr) = (None, None);
    while stdout.is_none() || stderr.is_none() {
        let timeout = command_deadline.saturating_duration_since(Instant::now());
        let (is_stdout, output) = receiver
            .recv_timeout(timeout)
            .map_err(|_| CaptureFailure::Deadline)?;
        let (bytes, exceeded_limit) =
            output.map_err(|error| CaptureFailure::Git(error.to_string()))?;
        if exceeded_limit {
            return Err(CaptureFailure::Git(
                "git command output exceeded its one MiB bound".into(),
            ));
        }
        if is_stdout {
            stdout = Some(bytes);
        } else {
            stderr = Some(bytes);
        }
    }
    let stdout = stdout.ok_or_else(|| CaptureFailure::Git("git stdout capture missing".into()))?;
    let stderr = stderr.ok_or_else(|| CaptureFailure::Git("git stderr capture missing".into()))?;
    let _ = stdout_reader.join();
    let _ = stderr_reader.join();
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

fn read_bounded(mut reader: impl std::io::Read) -> std::io::Result<(Vec<u8>, bool)> {
    let mut output = Vec::new();
    let mut buffer = [0u8; 8192];
    let mut exceeded_limit = false;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        let available = MAX_GIT_OUTPUT_BYTES.saturating_sub(output.len());
        let retained = read.min(available);
        output.extend_from_slice(&buffer[..retained]);
        exceeded_limit |= retained < read;
    }
    Ok((output, exceeded_limit))
}

pub fn capture_failure_text(error: CaptureFailure) -> String {
    match error {
        CaptureFailure::Deadline => "git checkpoint operation exceeded its deadline".into(),
        CaptureFailure::Git(error) => error,
    }
}

#[cfg(test)]
mod tests;
