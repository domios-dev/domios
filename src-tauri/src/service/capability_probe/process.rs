use super::protocol;
use codeterm_types::agent::capability_probe::{InitializeCapabilities, ProbeFailure};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

#[cfg(test)]
mod diagnostics;

const OUTPUT_LIMIT: u64 = 1_048_576;
pub(super) fn capture(
    binary: &Path,
    project: &Path,
    deadline: Duration,
) -> Result<InitializeCapabilities, ProbeFailure> {
    if !binary.is_absolute() {
        return Err(ProbeFailure::BinaryUnavailable);
    }
    #[cfg(windows)]
    if !binary
        .extension()
        .is_some_and(|extension| extension.to_string_lossy().eq_ignore_ascii_case("exe"))
    {
        return Err(ProbeFailure::BinaryUnavailable);
    }
    let mut command = Command::new(binary);
    command.args(protocol::args());
    capture_command(command, project, deadline)
}
pub(super) fn capture_command(
    mut command: Command,
    project: &Path,
    deadline: Duration,
) -> Result<InitializeCapabilities, ProbeFailure> {
    command
        .current_dir(project)
        .env_clear()
        .envs(std::env::vars_os().filter(|(key, _)| protocol::inherited_key_allowed(key)))
        .env("ENABLE_CLAUDEAI_MCP_SERVERS", "false")
        .env("CLAUDE_CODE_AUTO_CONNECT_IDE", "0")
        .env("CLAUDE_CODE_IDE_SKIP_AUTO_INSTALL", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(test)]
    command.stderr(Stdio::piped());
    let mut tree = super::super::provider_update::ProcessTree::prepare(&mut command)
        .map_err(|_| ProbeFailure::Spawn)?;
    let mut child = command.spawn().map_err(|_| ProbeFailure::Spawn)?;
    if tree.record_spawn(&child).is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err(ProbeFailure::Cleanup);
    }
    if tree.attach(&child).is_err() {
        let _ = tree.terminate(&mut child);
        let _ = child.wait();
        return Err(ProbeFailure::Cleanup);
    }
    #[cfg(test)]
    let diagnostic = diagnostics::Stderr::capture(&mut child);
    let (tx, rx) = mpsc::sync_channel(1);
    let stdout = child.stdout.take().expect("piped stdout");
    let reader = std::thread::Builder::new()
        .name("initialize-control-response".into())
        .spawn(move || {
            let mut stream = BufReader::new(stdout.take(OUTPUT_LIMIT + 1));
            let mut total = 0;
            loop {
                let mut line = String::new();
                let read = match stream.read_line(&mut line) {
                    Ok(0) => {
                        let _ = tx.send(Err(ProbeFailure::Protocol));
                        return;
                    }
                    Ok(read) => read,
                    Err(_) => {
                        let _ = tx.send(Err(ProbeFailure::Protocol));
                        return;
                    }
                };
                total += read;
                if total as u64 > OUTPUT_LIMIT {
                    let _ = tx.send(Err(ProbeFailure::OutputLimit));
                    return;
                }
                if let Ok(value) = serde_json::from_str(&line) {
                    if let Some(result) = protocol::decode(&value) {
                        let _ = tx.send(result);
                        return;
                    }
                }
            }
        });
    let result = match reader.as_ref() {
        Ok(_) => {
            let write = child
                .stdin
                .as_mut()
                .expect("piped stdin")
                .write_all(protocol::REQUEST);
            if write.is_err() {
                Err(ProbeFailure::Protocol)
            } else {
                rx.recv_timeout(deadline)
                    .unwrap_or(Err(ProbeFailure::Deadline))
            }
        }
        Err(_) => Err(ProbeFailure::Spawn),
    };
    #[cfg(test)]
    let before_cleanup = child.try_wait();
    #[cfg(all(test, windows, debug_assertions))]
    if result.is_err() {
        eprintln!(
            "probe child before cleanup: {}",
            codeterm_util::process_util::diagnostics::snapshot(&child)
        );
    }
    drop(child.stdin.take());
    if tree.terminate(&mut child).is_err()
        && !tree.observe_event().is_ok_and(|event| event.confirmed())
    {
        let _ = child.kill();
        let _ = child.wait();
        return Err(ProbeFailure::Cleanup);
    }
    let status = child.wait();
    #[cfg(test)]
    diagnostic.report(&command, &result, &before_cleanup, &status);
    let stopped = status.is_ok();
    tree.reaped(&child);
    #[cfg(windows)]
    let stopped = stopped
        && tree
            .wait_completion(Instant::now() + Duration::from_secs(5))
            .is_ok_and(|event| event.confirmed());
    #[cfg(not(windows))]
    let _ = Instant::now();
    if !stopped {
        return Err(ProbeFailure::Cleanup);
    }
    if let Ok(reader) = reader {
        let _ = reader.join();
    }
    result
}
