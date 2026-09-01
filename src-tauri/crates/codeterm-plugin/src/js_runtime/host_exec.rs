//! `host.exec` backend (host_api v2). Runs an allowlisted subprocess and returns
//! `{code, stdout, stderr}` as JSON. Permission (`subprocess.allow`) is enforced
//! by the caller via `PluginSandbox::can_exec` before this is invoked. Secrets
//! travel via `env`/`stdin`, never argv (visible to `ps`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::LazyLock;
use std::time::Duration;

use parking_lot::{Condvar, Mutex};
use serde::{Deserialize, Serialize};
use wait_timeout::ChildExt;

const MAX_EXEC_JOBS: usize = 128;

thread_local! {
    static HOST_EXEC_ENV: RefCell<Vec<HashMap<String, String>>> = const { RefCell::new(Vec::new()) };
}

pub(super) struct HostExecEnvGuard;

impl Drop for HostExecEnvGuard {
    fn drop(&mut self) {
        HOST_EXEC_ENV.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}

pub(super) fn host_env_scope(env: HashMap<String, String>) -> HostExecEnvGuard {
    HOST_EXEC_ENV.with(|stack| {
        stack.borrow_mut().push(env);
    });
    HostExecEnvGuard
}

pub(super) fn current_host_env() -> HashMap<String, String> {
    HOST_EXEC_ENV.with(|stack| stack.borrow().last().cloned().unwrap_or_default())
}

#[derive(Deserialize, Serialize)]
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

static RESOLVED_BINS: LazyLock<Mutex<HashMap<String, std::path::PathBuf>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// True when `p` is a regular file with an executable bit (any) set.
fn is_executable_file(p: &std::path::Path) -> bool {
    let Ok(meta) = std::fs::metadata(p) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        return meta.permissions().mode() & 0o111 != 0;
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Resolve `bin` to a runnable path, memoized per process. Ladder: (1) `which`
/// over the daemon PATH — unchanged when it hits; (2) on a miss, probe each
/// `extra_dirs` entry in order, returning the first `<dir>/<bin>` that is an
/// executable regular file; (3) fall back to the bare name (OS resolves at
/// spawn). The fallback only WIDENS discovery: probe paths are `dir.join(bin)`,
/// so the allowlisted basename is preserved by construction.
fn resolve_bin(bin: &str, extra_dirs: &[std::path::PathBuf]) -> std::path::PathBuf {
    if let Some(hit) = RESOLVED_BINS.lock().get(bin) {
        return hit.clone();
    }
    let resolved = resolve_bin_uncached(bin, extra_dirs);
    RESOLVED_BINS
        .lock()
        .insert(bin.to_string(), resolved.clone());
    resolved
}

fn resolve_bin_uncached(bin: &str, extra_dirs: &[std::path::PathBuf]) -> std::path::PathBuf {
    if let Ok(p) = which::which(bin) {
        return p;
    }
    for dir in extra_dirs {
        let candidate = dir.join(bin);
        if is_executable_file(&candidate) {
            debug_assert_eq!(
                candidate.file_name().and_then(|n| n.to_str()),
                Some(bin),
                "resolved basename must equal the requested allowlisted bin"
            );
            return candidate;
        }
    }
    std::path::PathBuf::from(bin)
}

/// Well-known dirs a GUI-launched app may miss from a login shell's PATH.
fn well_known_bin_dirs() -> Vec<std::path::PathBuf> {
    let mut dirs = vec![
        std::path::PathBuf::from("/opt/homebrew/bin"),
        std::path::PathBuf::from("/usr/local/bin"),
    ];
    if let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) {
        dirs.push(home.join(".npm-global/bin"));
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join("bin"));
        dirs.push(home.join(".volta/bin"));
    }
    dirs
}

/// Run the subprocess described by `opts_json`. The binary is already
/// permission-checked. Resolves the binary via `which`, falling back to the bare
/// name (PATH resolution by the OS).
pub(super) fn run(opts_json: &str) -> String {
    run_with_host_env(opts_json, &HashMap::new())
}

pub(super) fn run_with_host_env(opts_json: &str, host_env: &HashMap<String, String>) -> String {
    let opts: ExecOpts = match serde_json::from_str(opts_json) {
        Ok(o) => o,
        Err(e) => return err(&format!("bad exec opts: {e}")),
    };
    let opts = with_host_env(opts, host_env);
    let resolved = resolve_bin(&opts.bin, &well_known_bin_dirs());

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
        let mut command = Command::new(&resolved);
        codeterm_util::process_util::no_window_std(&mut command);
        return match command
            .args(&opts.args)
            .envs(&opts.env)
            .stdin(Stdio::null())
            .stdout(out)
            .stderr(errout)
            .spawn()
        {
            // running; the plugin manages its lifetime.
            Ok(child) => serde_json::json!({ "code": 0, "pid": child.id() }).to_string(),
            Err(e) => err(&format!("spawn {}: {e}", opts.bin)),
        };
    }

    let mut command = Command::new(&resolved);
    codeterm_util::process_util::no_window_std(&mut command);
    // Own process group so a timeout kill reaps descendants too — a grandchild
    // inheriting stdout/stderr would otherwise keep the reader threads blocked.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = match command
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
    // would block the child on write() while we wait, deadlocking until timeout.
    let (out_tx, out_rx) = std::sync::mpsc::channel::<String>();
    let (err_tx, err_rx) = std::sync::mpsc::channel::<String>();
    let _out_thread = child.stdout.take().map(|mut s| {
        std::thread::spawn(move || {
            let mut buf = String::new();
            let _ = s.read_to_string(&mut buf);
            let _ = out_tx.send(buf);
        })
    });
    let _err_thread = child.stderr.take().map(|mut s| {
        std::thread::spawn(move || {
            let mut buf = String::new();
            let _ = s.read_to_string(&mut buf);
            let _ = err_tx.send(buf);
        })
    });
    let timeout = Duration::from_millis(opts.timeout_ms.unwrap_or(30_000).max(1));
    let (status_code, timeout_error) = match child.wait_timeout(timeout) {
        Ok(Some(status)) => (status.code().unwrap_or(-1), None),
        Ok(None) => {
            terminate_child(&mut child);
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
    // After a timeout-kill, use a short SHARED drain deadline: orphaned
    // grandchildren (not reached by taskkill /T on Windows) keep pipe write-ends
    // open, which would block read_to_string indefinitely. One deadline across
    // both channels — sequential per-channel timeouts would otherwise add up
    // (2x500ms) and blow the caller's latency budget. Discard partial output.
    let drain = if timeout_error.is_some() {
        Duration::from_millis(400)
    } else {
        Duration::from_secs(60)
    };
    let deadline = std::time::Instant::now() + drain;
    let recv_until = |rx: &std::sync::mpsc::Receiver<String>| {
        let budget = deadline.saturating_duration_since(std::time::Instant::now());
        rx.recv_timeout(budget).unwrap_or_default()
    };
    let out = recv_until(&out_rx);
    let errout = recv_until(&err_rx);
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

#[cfg(windows)]
fn terminate_child(child: &mut Child) {
    let mut command = Command::new("taskkill");
    codeterm_util::process_util::no_window_std(&mut command);
    let killed_tree = command
        .args(["/PID", &child.id().to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if !killed_tree {
        let _ = child.kill();
    }
}

#[cfg(not(windows))]
fn terminate_child(child: &mut Child) {
    // Negative pid = the process group set at spawn; falls back to the direct
    // child if the group kill fails for any reason.
    let killed_group = unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL) } == 0;
    if !killed_group {
        let _ = child.kill();
    }
}

/// Extract the binary name from an exec-opts JSON blob, for the permission check.
pub(super) fn bin_of(opts_json: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(opts_json)
        .ok()
        .and_then(|v| v.get("bin").and_then(|b| b.as_str()).map(String::from))
}

// ── Async exec (job-id + poll) ──────────────────────────────────────────────

/// Completion signal for one job. A waiter blocks on the condvar; the worker
/// flips `done` and notifies. No polling and no elapsed-time check anywhere.
#[derive(Default)]
struct DoneEvent {
    done: Mutex<bool>,
    cv: Condvar,
}

impl DoneEvent {
    fn signal(&self) {
        *self.done.lock() = true;
        self.cv.notify_all();
    }

    fn wait(&self) {
        let mut done = self.done.lock();
        while !*done {
            self.cv.wait(&mut done);
        }
    }
}

#[derive(Default)]
struct ExecJob {
    plugin_id: String,
    done: bool,
    done_event: std::sync::Arc<DoneEvent>,
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
#[allow(dead_code)] // host-env-less convenience wrapper; kept symmetric with start_with_host_env
pub(super) fn start(plugin_id: &str, opts_json: &str) -> String {
    start_with_host_env(plugin_id, opts_json, &HashMap::new())
}

pub(super) fn start_with_host_env(
    plugin_id: &str,
    opts_json: &str,
    host_env: &HashMap<String, String>,
) -> String {
    if let Err(e) = serde_json::from_str::<ExecOpts>(opts_json) {
        return err(&format!("bad exec opts: {e}"));
    }

    let job_id = match start_exec_job(plugin_id) {
        Ok(job_id) => job_id,
        Err(e) => return err(&e),
    };

    let owned = match inject_host_env_json(opts_json, host_env) {
        Ok(json) => json,
        Err(e) => return err(&e),
    };
    let worker_job_id = job_id.clone();
    std::thread::spawn(move || run_exec_worker(worker_job_id, owned));

    serde_json::json!({ "jobId": job_id }).to_string()
}

fn inject_host_env_json(
    opts_json: &str,
    host_env: &HashMap<String, String>,
) -> Result<String, String> {
    let opts: ExecOpts =
        serde_json::from_str(opts_json).map_err(|e| format!("bad exec opts: {e}"))?;
    serde_json::to_string(&with_host_env(opts, host_env))
        .map_err(|e| format!("serialize exec opts: {e}"))
}

fn with_host_env(mut opts: ExecOpts, host_env: &HashMap<String, String>) -> ExecOpts {
    opts.env.extend(host_env.clone());
    opts
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
    EXEC_JOBS.lock().retain(|id, job| {
        let dropped = id == job_id && job.plugin_id == plugin_id;
        if dropped {
            job.done_event.signal();
        }
        !dropped
    });
}

pub(super) fn drop_plugin_jobs(plugin_id: &str) {
    let plugin_id = normalize_plugin_id(plugin_id);
    EXEC_JOBS.lock().retain(|_, job| {
        let dropped = job.plugin_id == plugin_id;
        if dropped {
            job.done_event.signal();
        }
        !dropped
    });
}

/// Block until `job_id` finishes, then hand back the payload `poll` would have
/// produced. The caller must hold no VM lock: this is the wait that used to sit
/// inside the plugin's JS frame, where it serialized every other call.
pub(super) fn wait_json(plugin_id: &str, job_id: &str) -> String {
    let plugin_id = normalize_plugin_id(plugin_id);
    let event = {
        let jobs = EXEC_JOBS.lock();
        match jobs.get(job_id).filter(|job| job.plugin_id == plugin_id) {
            Some(job) => job.done_event.clone(),
            None => {
                return serde_json::json!({ "done": true, "error": "job not found" }).to_string()
            }
        }
    };
    event.wait();
    poll(&plugin_id, job_id)
}

/// Test-only job whose completion is driven by `gate` instead of a subprocess,
/// so a blocking plugin call can be released by an event the test controls.
#[cfg(test)]
pub(super) fn start_gated_job<F>(plugin_id: &str, gate: F) -> String
where
    F: FnOnce() + Send + 'static,
{
    let job_id = match start_exec_job(plugin_id) {
        Ok(id) => id,
        Err(e) => return err(&e),
    };
    let worker = job_id.clone();
    std::thread::spawn(move || {
        gate();
        finish_exec_job(
            &worker,
            &serde_json::json!({ "code": 0, "stdout": "", "stderr": "" }),
        );
    });
    job_id
}

fn start_exec_job(plugin_id: &str) -> Result<String, String> {
    let plugin_id = normalize_plugin_id(plugin_id);
    let mut jobs = EXEC_JOBS.lock();
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
    let mut jobs = EXEC_JOBS.lock();
    if let Some(job) = jobs.get_mut(job_id) {
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
        job.done_event.signal();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::js_runtime::host_api::PluginSandbox;
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
            // A bounded failure deadline, not a performance budget: a genuine
            // hang still fails, while a loaded machine spawning a process no
            // longer does. Five seconds was the latter and failed under verify.
            assert!(
                started.elapsed() < Duration::from_secs(60),
                "exec timed out"
            );
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
    fn host_exec_env_overrides_plugin_env_for_identity() {
        let opts = serde_json::json!({
            "bin": "codeterm",
            "env": {
                "CODETERM_TAB_ID": "spoofed",
                "PLUGIN_VAR": "kept"
            }
        })
        .to_string();
        let host_env = HashMap::from([
            ("CODETERM_TAB_ID".to_string(), "chat-tab".to_string()),
            (
                "CODETERM_SOCKET_PATH".to_string(),
                "/tmp/codeterm.sock".to_string(),
            ),
        ]);

        let merged: ExecOpts =
            serde_json::from_str(&inject_host_env_json(&opts, &host_env).unwrap()).unwrap();

        assert_eq!(
            merged.env.get("CODETERM_TAB_ID").map(String::as_str),
            Some("chat-tab")
        );
        assert_eq!(
            merged.env.get("CODETERM_SOCKET_PATH").map(String::as_str),
            Some("/tmp/codeterm.sock")
        );
        assert_eq!(
            merged.env.get("PLUGIN_VAR").map(String::as_str),
            Some("kept")
        );
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
    fn completed_job_survives_an_unrelated_start_until_polled() {
        let _guard = TEST_LOCK.lock();
        reset_exec_jobs();
        let completed = start_exec_job("owner-plugin").unwrap();
        finish_exec_job(
            &completed,
            &serde_json::json!({ "code": 0, "stdout": "owned", "stderr": "" }),
        );

        let unrelated = start_exec_job("other-plugin").unwrap();
        let result: serde_json::Value =
            serde_json::from_str(&poll("owner-plugin", &completed)).unwrap();

        assert_eq!(result["code"], 0);
        assert_eq!(result["stdout"], "owned");
        close("other-plugin", &unrelated);
    }

    #[test]
    fn run_drains_output_larger_than_pipe_buffer_without_deadlock() {
        #[cfg(windows)]
        let opts = serde_json::json!({
            "bin": "powershell",
            "args": ["-NoProfile", "-Command", "[Console]::Out.Write('x' * 200000)"],
            "timeoutMs": 10000,
        });
        #[cfg(not(windows))]
        let opts = serde_json::json!({
            "bin": "sh",
            "args": ["-c", "head -c 200000 /dev/zero | tr '\\0' x"],
            "timeoutMs": 10000,
        });

        // A deadlocked pipe cannot reach the three assertions below: it blocks
        // until timeoutMs and comes back as an error with truncated output. That
        // is the proof. Wall-clock is not — it measured the machine's load, and
        // failed at 6.6s on a busy one while passing in 0.4s alone.
        let out: serde_json::Value = serde_json::from_str(&run(&opts.to_string())).unwrap();
        assert!(out.get("error").is_none(), "unexpected error: {out}");
        assert_eq!(out["code"], 0);
        assert_eq!(out["stdout"].as_str().map(str::len), Some(200_000));
    }

    #[test]
    fn resolve_bin_probes_fallback_dir_when_not_on_path() {
        // A bin absent from PATH must still resolve when its dir is passed as a
        // fallback candidate. RED today: resolve_bin ignores extra_dirs and
        // returns the bare name; GREEN once the fallback probe lands.
        let name = format!("bwprobe_{}", uuid::Uuid::new_v4().simple());
        let dir = std::env::temp_dir().join(format!(
            "host_exec_resolve_{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let bin_path = dir.join(&name);
        std::fs::write(&bin_path, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&bin_path).unwrap().permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&bin_path, perm).unwrap();
        }

        let resolved = resolve_bin(&name, std::slice::from_ref(&dir));
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(
            resolved, bin_path,
            "resolve_bin must find a bin via the provided fallback dir, not just PATH"
        );
    }

    #[test]
    fn resolve_bin_ignores_probe_dirs_when_which_succeeds() {
        let _guard = TEST_LOCK.lock();
        let mut process_env = codeterm_util::test_env::EnvGuard::lock();
        RESOLVED_BINS.lock().clear();
        #[cfg(windows)]
        let name = format!("host_exec_{}.cmd", uuid::Uuid::new_v4().simple());
        #[cfg(not(windows))]
        let name = format!("host_exec_{}", uuid::Uuid::new_v4().simple());

        let path_dir =
            std::env::temp_dir().join(format!("host_exec_path_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&path_dir).unwrap();
        let expected = path_dir.join(&name);
        std::fs::write(&expected, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&expected).unwrap().permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&expected, perm).unwrap();
        }

        let old_path = std::env::var_os("PATH");
        let mut paths = vec![path_dir.clone()];
        if let Some(old) = old_path.as_deref() {
            paths.extend(std::env::split_paths(old));
        }
        process_env.set("PATH", std::env::join_paths(paths).unwrap());

        // Decoy dir holding a same-named executable must be ignored: which() wins.
        let dir =
            std::env::temp_dir().join(format!("host_exec_decoy_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let decoy = dir.join(&name);
        std::fs::write(&decoy, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&decoy).unwrap().permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&decoy, perm).unwrap();
        }

        let resolved = resolve_bin(&name, std::slice::from_ref(&dir));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&path_dir);
        RESOLVED_BINS.lock().clear();
        assert_eq!(
            resolved, expected,
            "which() result must win over probe dirs"
        );
    }

    #[test]
    fn resolve_bin_via_probe_preserves_basename() {
        let _guard = TEST_LOCK.lock();
        RESOLVED_BINS.lock().clear();
        let name = format!("bwbase_{}", uuid::Uuid::new_v4().simple());
        let dir =
            std::env::temp_dir().join(format!("host_exec_base_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        let bin_path = dir.join(&name);
        std::fs::write(&bin_path, b"#!/bin/sh\nexit 0\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = std::fs::metadata(&bin_path).unwrap().permissions();
            perm.set_mode(0o755);
            std::fs::set_permissions(&bin_path, perm).unwrap();
        }

        let resolved = resolve_bin(&name, std::slice::from_ref(&dir));
        let _ = std::fs::remove_dir_all(&dir);
        RESOLVED_BINS.lock().clear();
        assert_eq!(
            resolved.file_name().and_then(|n| n.to_str()),
            Some(name.as_str()),
            "a probe-dir resolution must preserve the requested basename"
        );
    }

    #[test]
    fn run_times_out_blocking_process() {
        // The grandchild outsleeps any PowerShell startup jitter, so run()
        // blocking on it reads as a 30s outlier, not a boundary value.
        #[cfg(windows)]
        let opts = serde_json::json!({
            "bin": "powershell",
            "args": [
                "-NoProfile",
                "-Command",
                r#"$p = Start-Process -FilePath powershell -ArgumentList @('-NoProfile','-Command','Start-Sleep -Seconds 30') -NoNewWindow -PassThru; Wait-Process -Id $p.Id"#
            ],
            "timeoutMs": 50,
        });
        #[cfg(not(windows))]
        let opts = serde_json::json!({
            "bin": "sh",
            "args": ["-c", "sleep 2"],
            "timeoutMs": 50,
        });

        // Windows: budget for slow PowerShell interpreter startup under CI
        // contention (measured well before the 30s a real regression would take).
        #[cfg(windows)]
        let max_elapsed = Duration::from_secs(8);
        #[cfg(not(windows))]
        let max_elapsed = Duration::from_secs(1);

        let started = Instant::now();
        let out: serde_json::Value = serde_json::from_str(&run(&opts.to_string())).unwrap();
        assert!(
            started.elapsed() < max_elapsed,
            "timeout waited for descendant pipe handles to close: {:?}",
            started.elapsed()
        );
        assert!(out
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .contains("timed out"));
    }
}
