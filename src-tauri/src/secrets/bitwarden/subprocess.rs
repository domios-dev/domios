//! `SubprocessBwExec` — production exec backed by `tokio::process`.

use async_trait::async_trait;
use tokio::process::Command;

use super::exec::{BwExec, BwResponse, BwRunCtx};

pub struct SubprocessBwExec {
    bw_path: std::path::PathBuf,
}

impl SubprocessBwExec {
    pub fn new(bw_path: std::path::PathBuf) -> Self {
        Self { bw_path }
    }

    /// Resolve the `bw` binary. Tauri apps launched from Finder/launchd
    /// inherit a minimal PATH (`/usr/bin:/bin:/usr/sbin:/sbin`) — Homebrew,
    /// npm-global, scoop, and friends are NOT in it, so `which::which`
    /// alone returns `None` even when `bw` is installed. We therefore fall
    /// back to a platform-specific list of well-known install paths.
    pub fn detect() -> Option<Self> {
        first_executable(detection_candidates()).map(Self::new)
    }
}

/// Iterator of `bw` binary candidates, most-likely-correct first.
/// Order: process PATH (cheapest, honors user override) → platform
/// fallbacks (covers Tauri's inherited-PATH gap).
fn detection_candidates() -> Vec<std::path::PathBuf> {
    let mut out = Vec::with_capacity(8);
    if let Ok(p) = which::which("bw") {
        out.push(p);
    }
    out.extend(fallback_bw_paths());
    out
}

/// Platform-specific install locations to probe when `bw` isn't on PATH.
pub(super) fn fallback_bw_paths() -> Vec<std::path::PathBuf> {
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    let home = dirs::home_dir();

    #[cfg(target_os = "macos")]
    {
        paths.push(std::path::PathBuf::from("/opt/homebrew/bin/bw"));
        paths.push(std::path::PathBuf::from("/usr/local/bin/bw"));
        paths.push(std::path::PathBuf::from(
            "/opt/homebrew/opt/bitwarden-cli/bin/bw",
        ));
        if let Some(h) = home.as_ref() {
            paths.push(h.join(".local/bin/bw"));
            paths.push(h.join(".npm-global/bin/bw"));
            paths.push(h.join(".bun/bin/bw"));
        }
    }
    #[cfg(target_os = "linux")]
    {
        paths.push(std::path::PathBuf::from("/usr/local/bin/bw"));
        paths.push(std::path::PathBuf::from("/usr/bin/bw"));
        paths.push(std::path::PathBuf::from("/snap/bin/bw"));
        if let Some(h) = home.as_ref() {
            paths.push(h.join(".local/bin/bw"));
            paths.push(h.join(".npm-global/bin/bw"));
        }
    }
    #[cfg(windows)]
    {
        if let Ok(la) = std::env::var("LOCALAPPDATA") {
            paths.push(std::path::PathBuf::from(la).join("Programs\\Bitwarden CLI\\bw.exe"));
        }
        if let Ok(pf) = std::env::var("ProgramFiles") {
            paths.push(std::path::PathBuf::from(pf).join("Bitwarden CLI\\bw.exe"));
        }
        if let Ok(ad) = std::env::var("APPDATA") {
            paths.push(std::path::PathBuf::from(ad).join("npm\\bw.cmd"));
        }
        if let Ok(up) = std::env::var("USERPROFILE") {
            paths.push(
                std::path::PathBuf::from(up).join("scoop\\apps\\bitwarden-cli\\current\\bw.exe"),
            );
        }
        let _ = home;
    }
    paths
}

pub(super) fn first_executable<I: IntoIterator<Item = std::path::PathBuf>>(
    candidates: I,
) -> Option<std::path::PathBuf> {
    candidates.into_iter().find(|p| is_executable_file(p))
}

#[cfg(unix)]
pub(super) fn is_executable_file(path: &std::path::Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match path.metadata() {
        Ok(md) => md.is_file() && md.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

#[cfg(windows)]
pub(super) fn is_executable_file(path: &std::path::Path) -> bool {
    path.is_file()
}

#[async_trait]
impl BwExec for SubprocessBwExec {
    async fn run(&self, ctx: BwRunCtx<'_>) -> Result<BwResponse, String> {
        use tokio::io::AsyncWriteExt;

        let cmd_label = ctx.args.first().copied().unwrap_or("?").to_string();
        let mut cmd = Command::new(&self.bw_path);
        cmd.args(&ctx.args);
        cmd.arg("--nointeraction").arg("--response");
        cmd.envs(&ctx.env);
        if let Some(session) = ctx.session {
            cmd.env("BW_SESSION", session);
        }
        cmd.stdin(std::process::Stdio::piped());
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
        crate::process_util::no_window_tokio(&mut cmd);

        let mut child = cmd.spawn().map_err(|e| format!("spawn bw: {}", e))?;
        if let Some(stdin) = ctx.stdin {
            if let Some(mut sin) = child.stdin.take() {
                sin.write_all(stdin.as_bytes())
                    .await
                    .map_err(|e| format!("write stdin: {}", e))?;
            }
        }
        let output = child
            .wait_with_output()
            .await
            .map_err(|e| format!("wait bw: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        // With --response, bw prints the wrapper JSON to stdout for both success and failure. If parsing fails, fall through to a raw Backend error so
        // the caller sees something useful instead of silently treating it as success.
        if let Ok(resp) = serde_json::from_str::<BwResponse>(stdout.trim()) {
            if !resp.success {
                log::warn!(
                    "[secrets/bw] {} failed: msg={:?} errorCode={:?} stderr={}",
                    cmd_label,
                    resp.message,
                    resp.error_code,
                    stderr.trim()
                );
            }
            return Ok(resp);
        }

        let combined = if !stderr.is_empty() {
            stderr.clone()
        } else if !stdout.is_empty() {
            stdout.clone()
        } else {
            format!("bw exited with {}", output.status)
        };
        log::warn!(
            "[secrets/bw] {} produced unparseable output: status={} stdout={} stderr={}",
            cmd_label,
            output.status,
            stdout.trim(),
            stderr.trim()
        );
        Ok(BwResponse {
            success: false,
            data: serde_json::Value::Null,
            message: Some(combined),
            error_code: None,
        })
    }
}
