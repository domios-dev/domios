use codeterm_plugin::provider::AuthStatusProbe;
use serde_json::Value;
use tokio::io::AsyncReadExt;

#[cfg(test)]
pub(super) fn capture(binary: &str, probe: &AuthStatusProbe) -> Option<Value> {
    capture_in(binary, &[], probe)
}

pub(super) fn capture_in(
    binary: &str,
    extra_dirs: &[std::path::PathBuf],
    probe: &AuthStatusProbe,
) -> Option<Value> {
    capture_result_in(binary, extra_dirs, probe).ok()
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProbeFailure {
    TimedOut,
    Unavailable,
}

fn probe_timeout(timeout_ms: u64) -> std::time::Duration {
    std::time::Duration::from_millis(timeout_ms.clamp(1, 30_000))
}

async fn await_probe(
    timeout_ms: u64,
    probe: impl std::future::Future<Output = Option<Value>>,
) -> Result<Value, ProbeFailure> {
    tokio::time::timeout(probe_timeout(timeout_ms), probe)
        .await
        .map_err(|_| ProbeFailure::TimedOut)?
        .ok_or(ProbeFailure::Unavailable)
}

pub fn capture_result_in(
    binary: &str,
    extra_dirs: &[std::path::PathBuf],
    probe: &AuthStatusProbe,
) -> Result<Value, ProbeFailure> {
    let codeterm_plugin::js_runtime::BinResolution::Found(binary) =
        codeterm_plugin::js_runtime::probe_bin_with_extra_dirs(binary, extra_dirs).resolution
    else {
        return Err(ProbeFailure::Unavailable);
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| ProbeFailure::Unavailable)?;
    runtime.block_on(async {
        await_probe(probe.timeout_ms, async {
            let command = codeterm_plugin::provider_command::command(&binary).ok()?;
            let mut command = tokio::process::Command::from(command);
            codeterm_util::process_util::no_window_std(command.as_std_mut());
            let mut child = command
                .args(&probe.args)
                .kill_on_drop(true)
                .stdin(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .ok()?;
            let stdout = child.stdout.take()?;
            let read = async {
                let mut text = String::new();
                stdout.take(65_537).read_to_string(&mut text).await.ok()?;
                (text.len() <= 65_536).then_some(text)
            };
            let (text, status) = tokio::join!(read, child.wait());
            if !matches!(status.ok()?.code(), Some(0 | 1)) {
                return None;
            }
            serde_json::from_str(&text?).ok()
        })
        .await
    })
}

#[cfg(test)]
mod tests;
