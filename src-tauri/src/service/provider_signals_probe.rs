use codeterm_plugin::provider::AuthStatusProbe;
use serde_json::Value;
use tokio::io::AsyncReadExt;

pub(super) fn capture(binary: &str, probe: &AuthStatusProbe) -> Option<Value> {
    let binary = codeterm_plugin::js_runtime::resolved_bin_path(binary)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    runtime.block_on(async {
        tokio::time::timeout(
            std::time::Duration::from_millis(probe.timeout_ms.clamp(1, 5000)),
            async {
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
            },
        )
        .await
        .ok()
        .flatten()
    })
}
