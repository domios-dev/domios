//! `host.exec` backend (host_api v2). Runs an allowlisted subprocess and returns
//! `{code, stdout, stderr}` as JSON. Permission (`subprocess.allow`) is enforced
//! by the caller via `PluginSandbox::can_exec` before this is invoked. Secrets
//! travel via `env`/`stdin`, never argv (visible to `ps`).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde::Deserialize;
use wait_timeout::ChildExt;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExecOpts {
    bin: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    #[serde(default)]
    stdin: Option<String>,
    /// Spawn a long-lived process detached: don't wait for exit, return
    /// `{code:0, pid}` immediately. Lets a plugin own a daemon (e.g. a transcribe
    /// server) without blocking the VM. The plugin stops it itself (e.g. `pkill`).
    #[serde(default)]
    detach: bool,
    /// When detached, append stdout+stderr here; otherwise they're discarded.
    #[serde(default)]
    log_file: Option<String>,
    /// Kill blocking subprocesses after this many milliseconds.
    #[serde(default)]
    timeout_ms: Option<u64>,
}

fn err(msg: &str) -> String {
    serde_json::json!({ "error": msg }).to_string()
}

/// Run the subprocess described by `opts_json`. The binary is already
/// permission-checked. Resolves the binary via `which`, falling back to the bare
/// name (PATH resolution by the OS).
pub(super) fn run(opts_json: &str) -> String {
    let opts: ExecOpts = match serde_json::from_str(opts_json) {
        Ok(o) => o,
        Err(e) => return err(&format!("bad exec opts: {e}")),
    };
    let resolved = which::which(&opts.bin).unwrap_or_else(|_| std::path::PathBuf::from(&opts.bin));

    if opts.detach {
        let log = |p: &str| {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)
                .ok()
        };
        let (out, errout) = match opts.log_file.as_deref().and_then(log) {
            Some(f) => match f.try_clone() {
                Ok(f2) => (Stdio::from(f), Stdio::from(f2)),
                Err(_) => (Stdio::from(f), Stdio::null()),
            },
            None => (Stdio::null(), Stdio::null()),
        };
        return match Command::new(&resolved)
            .args(&opts.args)
            .envs(&opts.env)
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(errout)
            .spawn()
        {
            // Drop the handle without waiting → the process is detached and keeps
            // running; the plugin manages its lifetime.
            Ok(child) => serde_json::json!({ "code": 0, "pid": child.id() }).to_string(),
            Err(e) => err(&format!("spawn {}: {e}", opts.bin)),
        };
    }

    let mut child = match Command::new(&resolved)
        .args(&opts.args)
        .envs(&opts.env)
        .stdin(if opts.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return err(&format!("spawn {}: {e}", opts.bin)),
    };
    if let Some(s) = &opts.stdin {
        if let Some(mut si) = child.stdin.take() {
            let _ = si.write_all(s.as_bytes());
        }
    }
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let timeout = Duration::from_millis(opts.timeout_ms.unwrap_or(30_000).max(1));
    let (status_code, timeout_error) = match child.wait_timeout(timeout) {
        Ok(Some(status)) => (status.code().unwrap_or(-1), None),
        Ok(None) => {
            let _ = child.kill();
            let status = child.wait().ok();
            (
                status.and_then(|s| s.code()).unwrap_or(-1),
                Some(format!(
                    "{} timed out after {}ms",
                    opts.bin,
                    timeout.as_millis()
                )),
            )
        }
        Err(e) => return err(&format!("wait {}: {e}", opts.bin)),
    };
    let mut out = String::new();
    if let Some(mut s) = stdout.take() {
        let _ = s.read_to_string(&mut out);
    }
    let mut errout = String::new();
    if let Some(mut s) = stderr.take() {
        let _ = s.read_to_string(&mut errout);
    }
    let mut value = serde_json::json!({
        "code": status_code,
        "stdout": out,
        "stderr": errout,
    });
    if let Some(e) = timeout_error {
        value["error"] = serde_json::json!(e);
    }
    value.to_string()
}

/// Extract the binary name from an exec-opts JSON blob, for the permission check.
pub(super) fn bin_of(opts_json: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(opts_json)
        .ok()
        .and_then(|v| v.get("bin").and_then(|b| b.as_str()).map(String::from))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_times_out_blocking_process() {
        #[cfg(windows)]
        let opts = serde_json::json!({
            "bin": "powershell",
            "args": ["-NoProfile", "-Command", "Start-Sleep -Seconds 2"],
            "timeoutMs": 50,
        });
        #[cfg(not(windows))]
        let opts = serde_json::json!({
            "bin": "sh",
            "args": ["-c", "sleep 2"],
            "timeoutMs": 50,
        });

        let out: serde_json::Value = serde_json::from_str(&run(&opts.to_string())).unwrap();
        assert!(out
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .contains("timed out"));
    }
}
