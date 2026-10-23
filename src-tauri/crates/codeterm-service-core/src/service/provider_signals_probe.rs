use codeterm_plugin::provider::AuthStatusProbe;
use serde_json::Value;
use tokio::io::AsyncReadExt;

pub(super) fn capture(binary: &str, probe: &AuthStatusProbe) -> Option<Value> {
    capture_result(binary, probe).ok()
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ProbeFailure {
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

pub(super) fn capture_result(binary: &str, probe: &AuthStatusProbe) -> Result<Value, ProbeFailure> {
    let codeterm_plugin::js_runtime::BinResolution::Found(binary) =
        codeterm_plugin::js_runtime::probe_bin(binary).resolution
    else {
        return Err(ProbeFailure::Unavailable);
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| ProbeFailure::Unavailable)?;
    runtime.block_on(async {
        await_probe(probe.timeout_ms, async {
            let mut command = tokio::process::Command::new(binary);
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
