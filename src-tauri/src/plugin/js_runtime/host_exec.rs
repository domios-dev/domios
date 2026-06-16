//! `host.exec` backend (host_api v2). Runs an allowlisted subprocess and returns
//! `{code, stdout, stderr}` as JSON. Permission (`subprocess.allow`) is enforced
//! by the caller via `PluginSandbox::can_exec` before this is invoked. Secrets
//! travel via `env`/`stdin`, never argv (visible to `ps`).

use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

use serde::Deserialize;

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
    let resolved =
        which::which(&opts.bin).unwrap_or_else(|_| std::path::PathBuf::from(&opts.bin));
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
    match child.wait_with_output() {
        Ok(out) => serde_json::json!({
            "code": out.status.code().unwrap_or(-1),
            "stdout": String::from_utf8_lossy(&out.stdout),
            "stderr": String::from_utf8_lossy(&out.stderr),
        })
        .to_string(),
        Err(e) => err(&format!("wait {}: {e}", opts.bin)),
    }
}

/// Extract the binary name from an exec-opts JSON blob, for the permission check.
pub(super) fn bin_of(opts_json: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(opts_json)
        .ok()
        .and_then(|v| v.get("bin").and_then(|b| b.as_str()).map(String::from))
}
