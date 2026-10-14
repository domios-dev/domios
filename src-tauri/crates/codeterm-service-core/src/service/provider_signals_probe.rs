use codeterm_plugin::provider::AuthStatusProbe;
use serde_json::Value;
use tokio::io::AsyncReadExt;

pub(super) fn capture(binary: &str, probe: &AuthStatusProbe) -> Option<Value> {
    let codeterm_plugin::js_runtime::BinResolution::Found(binary) =
        codeterm_plugin::js_runtime::probe_bin(binary).resolution
    else {
        return None;
    };
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

#[cfg(test)]
mod tests {
    use super::capture;
    use codeterm_plugin::provider::AuthStatusProbe;

    #[test]
    fn capture_probes_a_binary_no_provider_listing_has_cached() {
        let dir = tempfile::tempdir().unwrap();
        #[cfg(windows)]
        let (bin_path, body) = (
            dir.path().join("auth-status.cmd"),
            "@echo {\"signedIn\":true}\r\n",
        );
        #[cfg(not(windows))]
        let (bin_path, body) = (
            dir.path().join("auth-status"),
            "#!/bin/sh\necho '{\"signedIn\":true}'\n",
        );
        std::fs::write(&bin_path, body).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&bin_path).unwrap().permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&bin_path, perm).unwrap();
        }
        let bin = bin_path.to_string_lossy().into_owned();
        codeterm_plugin::js_runtime::forget_resolved_bin(&bin);
        let probe = AuthStatusProbe {
            args: Vec::new(),
            timeout_ms: 5000,
        };

        let captured = capture(&bin, &probe);
        codeterm_plugin::js_runtime::forget_resolved_bin(&bin);

        assert_eq!(captured, Some(serde_json::json!({ "signedIn": true })));
    }
}
