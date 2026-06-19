//! `host.exec` backend (host_api v2). Runs an allowlisted subprocess and returns
//! `{code, stdout, stderr}` as JSON. Permission (`subprocess.allow`) is enforced
//! by the caller via `PluginSandbox::can_exec` before this is invoked. Secrets
//! travel via `env`/`stdin`, never argv (visible to `ps`).

use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::time::Duration;

use parking_lot::Mutex;
use serde::Deserialize;
use wait_timeout::ChildExt;

const MAX_EXEC_JOBS: usize = 128;

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

// ── Async exec (job-id + poll) ──────────────────────────────────────────────
// Mirrors `host.fetch.stream`/`streamPoll`/`streamClose` in host_net.rs. The
// blocking `run` above holds the per-plugin VM mutex for the command's whole
// duration — one slow tool exec freezes every sibling pane sharing the VM. This
// path spawns the command on a background thread and buffers the terminal
// result, so the VM lock is held only for the sub-ms `start`/`poll` calls.

#[derive(Default)]
struct ExecJob {
    plugin_id: String,
    done: bool,
    code: Option<i32>,
    stdout: Option<String>,
    stderr: Option<String>,
    error: Option<String>,
}

static EXEC_JOBS: LazyLock<Mutex<HashMap<String, ExecJob>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn normalize_plugin_id(plugin_id: &str) -> String {
    if plugin_id.is_empty() {
        "anonymous".to_string()
    } else {
        plugin_id.to_string()
    }
}

/// Start an async exec. The caller must run the same `subprocess.allow` gate
/// (`PluginSandbox::can_exec`) that the blocking `run` callsite applies.
pub(super) fn start(plugin_id: &str, opts_json: &str) -> String {
    if let Err(e) = serde_json::from_str::<ExecOpts>(opts_json) {
        return err(&format!("bad exec opts: {e}"));
    }

    let job_id = match start_exec_job(plugin_id) {
        Ok(job_id) => job_id,
        Err(e) => return err(&e),
    };

    let owned = opts_json.to_string();
    let worker_job_id = job_id.clone();
    std::thread::spawn(move || run_exec_worker(worker_job_id, owned));

    serde_json::json!({ "jobId": job_id }).to_string()
}

pub(super) fn poll(plugin_id: &str, job_id: &str) -> String {
    let plugin_id = normalize_plugin_id(plugin_id);
    let mut jobs = EXEC_JOBS.lock();
    let Some(job) = jobs
        .get_mut(job_id)
        .filter(|job| job.plugin_id == plugin_id)
    else {
        return serde_json::json!({ "done": true, "error": "job not found" }).to_string();
    };
    if !job.done {
        return serde_json::json!({ "done": false }).to_string();
    }

    let mut out = serde_json::Map::new();
    out.insert("done".to_string(), serde_json::json!(true));
    if let Some(code) = job.code {
        out.insert("code".to_string(), serde_json::json!(code));
    }
    if let Some(stdout) = &job.stdout {
        out.insert("stdout".to_string(), serde_json::json!(stdout));
    }
    if let Some(stderr) = &job.stderr {
        out.insert("stderr".to_string(), serde_json::json!(stderr));
    }
    if let Some(error) = &job.error {
        out.insert("error".to_string(), serde_json::json!(error));
    }
    jobs.remove(job_id);
    serde_json::Value::Object(out).to_string()
}

pub(super) fn close(plugin_id: &str, job_id: &str) {
    let plugin_id = normalize_plugin_id(plugin_id);
    EXEC_JOBS
        .lock()
        .retain(|id, job| id != job_id || job.plugin_id != plugin_id);
}

pub(super) fn drop_plugin_jobs(plugin_id: &str) {
    let plugin_id = normalize_plugin_id(plugin_id);
    EXEC_JOBS.lock().retain(|_, job| job.plugin_id != plugin_id);
}

fn start_exec_job(plugin_id: &str) -> Result<String, String> {
    let plugin_id = normalize_plugin_id(plugin_id);
    let mut jobs = EXEC_JOBS.lock();
    jobs.retain(|_, job| !job.done);
    if jobs.len() >= MAX_EXEC_JOBS {
        return Err("too many exec jobs".to_string());
    }

    let job_id = format!("exec:{plugin_id}:{}", uuid::Uuid::new_v4().simple());
    jobs.insert(
        job_id.clone(),
        ExecJob {
            plugin_id,
            ..ExecJob::default()
        },
    );
    Ok(job_id)
}

fn run_exec_worker(job_id: String, opts_json: String) {
    let value = match std::panic::catch_unwind(|| run(&opts_json)) {
        Ok(s) => serde_json::from_str::<serde_json::Value>(&s)
            .unwrap_or_else(|_| serde_json::json!({ "error": "exec produced invalid json" })),
        Err(_) => serde_json::json!({ "error": "exec worker panicked" }),
    };
    finish_exec_job(&job_id, &value);
}

fn finish_exec_job(job_id: &str, value: &serde_json::Value) {
    if let Some(job) = EXEC_JOBS.lock().get_mut(job_id) {
        job.done = true;
        job.code = value.get("code").and_then(|v| v.as_i64()).map(|c| c as i32);
        job.stdout = value
            .get("stdout")
            .and_then(|v| v.as_str())
            .map(String::from);
        job.stderr = value
            .get("stderr")
            .and_then(|v| v.as_str())
            .map(String::from);
        job.error = value
            .get("error")
            .and_then(|v| v.as_str())
            .map(String::from);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::js_runtime::host_api::PluginSandbox;
    use std::sync::LazyLock;
    use std::time::Instant;

    static TEST_LOCK: LazyLock<parking_lot::Mutex<()>> =
        LazyLock::new(|| parking_lot::Mutex::new(()));

    fn reset_exec_jobs() {
        EXEC_JOBS.lock().clear();
    }

    fn print_hello_opts() -> serde_json::Value {
        #[cfg(windows)]
        {
            serde_json::json!({
                "bin": "powershell",
                "args": ["-NoProfile", "-Command", "[Console]::Out.Write('hello')"],
            })
        }
        #[cfg(not(windows))]
        {
            serde_json::json!({ "bin": "sh", "args": ["-c", "printf hello"] })
        }
    }

    fn sleep_opts() -> serde_json::Value {
        #[cfg(windows)]
        {
            serde_json::json!({
                "bin": "powershell",
                "args": ["-NoProfile", "-Command", "Start-Sleep -Seconds 2"],
            })
        }
        #[cfg(not(windows))]
        {
            serde_json::json!({ "bin": "sh", "args": ["-c", "sleep 2"] })
        }
    }

    fn poll_until_done(plugin_id: &str, job_id: &str) -> serde_json::Value {
        let started = Instant::now();
        loop {
            let v: serde_json::Value = serde_json::from_str(&poll(plugin_id, job_id)).unwrap();
            if v["done"].as_bool() == Some(true) {
                return v;
            }
            assert!(started.elapsed() < Duration::from_secs(5), "exec timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn start_then_poll_yields_done_and_stdout() {
        let _guard = TEST_LOCK.lock();
        reset_exec_jobs();
        let started: serde_json::Value =
            serde_json::from_str(&start("test-plugin", &print_hello_opts().to_string())).unwrap();
        let job_id = started["jobId"].as_str().expect("job id");
        assert!(job_id.starts_with("exec:test-plugin:"));

        let done = poll_until_done("test-plugin", job_id);
        assert_eq!(done["done"], true);
        assert_eq!(done["code"], 0);
        assert_eq!(done["stdout"], "hello");

        // job is removed once the terminal result has been delivered
        let missing: serde_json::Value =
            serde_json::from_str(&poll("test-plugin", job_id)).unwrap();
        assert_eq!(missing["error"], "job not found");
    }

    #[test]
    fn non_allowlisted_cmd_is_denied_by_same_gate_as_blocking_exec() {
        let _guard = TEST_LOCK.lock();
        reset_exec_jobs();
        let sb = PluginSandbox::restricted(vec![], vec![]);
        let opts = serde_json::json!({ "bin": "rm", "args": ["-rf", "/"] }).to_string();
        let bin = bin_of(&opts).unwrap_or_default();

        let raw = if sb.can_exec(&bin) {
            start("test-plugin", &opts)
        } else {
            err(&format!("exec denied: {bin}"))
        };

        let denied: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(denied["error"], "exec denied: rm");
    }

    #[test]
    fn start_returns_immediately_without_holding_the_caller_for_the_command_duration() {
        let _guard = TEST_LOCK.lock();
        reset_exec_jobs();
        let t = Instant::now();
        let started: serde_json::Value =
            serde_json::from_str(&start("slow-plugin", &sleep_opts().to_string())).unwrap();
        let elapsed = t.elapsed();
        let job_id = started["jobId"].as_str().expect("job id");

        // The whole point: start spawns the command on a background thread and
        // returns sub-ms, so the VM lock held across this call is released long
        // before the 2s command finishes.
        assert!(
            elapsed < Duration::from_millis(500),
            "start blocked for {elapsed:?} — VM lock would be held for the exec duration"
        );

        let poll1: serde_json::Value = serde_json::from_str(&poll("slow-plugin", job_id)).unwrap();
        assert_eq!(poll1["done"], false);

        close("slow-plugin", job_id);
    }

    #[test]
    fn exec_job_count_is_capped() {
        let _guard = TEST_LOCK.lock();
        reset_exec_jobs();
        for _ in 0..MAX_EXEC_JOBS {
            start_exec_job("cap-plugin").unwrap();
        }
        assert_eq!(
            start_exec_job("cap-plugin").unwrap_err(),
            "too many exec jobs"
        );
        drop_plugin_jobs("cap-plugin");
    }

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
