//! `host.exec` backend (host_api v2). Runs an allowlisted subprocess and returns
//! `{code, stdout, stderr}` as JSON. Permission (`subprocess.allow`) is enforced
//! by the caller via `PluginSandbox::can_exec` before this is invoked. Secrets
//! travel via `env`/`stdin`, never argv (visible to `ps`).

mod protocol;
use protocol::{ExecPollResult, ExecResult};

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::Stdio;
use std::sync::{Arc, LazyLock};
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};
use serde::{Deserialize, Serialize};
use wait_timeout::ChildExt;

use super::bin_resolve::{resolve_bin, well_known_bin_dirs};
use super::host_api::PluginSandbox;

const MAX_EXEC_JOBS_PER_PLUGIN: usize = 128;
const EXEC_JOB_RETENTION: Duration = Duration::from_secs(5 * 60);
const ASYNC_JOB_WAIT_BOUND: Duration = super::call_deadline::STANDARD;
mod child;
mod control;
mod scoped;
pub(super) use scoped::{
    abort_scoped_json_job, drop_scoped_json_jobs, start_admitted_scoped_json_job,
    start_scoped_json_job,
};

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

#[derive(Deserialize, Serialize, ts_rs::TS)]
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
    #[serde(default)]
    conversation: Option<Vec<ExecConversationStep>>,
}

#[derive(Deserialize, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
struct ExecConversationStep {
    stdin: String,
    await_response_ids: Vec<u64>,
}

fn err(msg: &str) -> String {
    serde_json::to_string(&codeterm_wire::plugin_host::HostError { error: msg.into() })
        .unwrap_or_else(|_| "null".into())
}

/// Run the subprocess described by `opts_json`. The binary is already
/// permission-checked. Resolves the binary through the shared current PATH
/// ladder, falling back to the bare name for the OS to resolve.
#[cfg(test)]
pub(super) fn run(opts_json: &str) -> String {
    run_with_host_env(opts_json, &HashMap::new(), &PluginSandbox::unrestricted())
}

pub(super) fn run_with_host_env(
    opts_json: &str,
    host_env: &HashMap<String, String>,
    sandbox: &PluginSandbox,
) -> String {
    let opts: ExecOpts = match serde_json::from_str(opts_json) {
        Ok(o) => o,
        Err(e) => return err(&format!("bad exec opts: {e}")),
    };
    let opts = with_host_env(opts, host_env);
    if let Err(error) = validate_detached_log(&opts, sandbox) {
        return err(&error);
    }
    if let Some(conversation) = opts.conversation.as_deref() {
        return run_conversation(&opts, conversation);
    }
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
        let mut command = match crate::provider_command::command(&resolved) {
            Ok(command) => command,
            Err(error) => return err(&error),
        };
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
            Ok(child) => serde_json::to_string(&ExecResult {
                code: Some(0),
                pid: Some(child.id()),
                ..Default::default()
            })
            .unwrap_or_else(|_| "null".into()),
            Err(e) => err(&format!("spawn {}: {e}", opts.bin)),
        };
    }

    let mut command = match crate::provider_command::command(&resolved) {
        Ok(command) => command,
        Err(error) => return err(&error),
    };
    let mut child = match child::Owned::spawn(
        command
            .args(&opts.args)
            .envs(&opts.env)
            .stdin(if opts.stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    ) {
        Ok(c) => c,
        Err(e) => return err(&format!("spawn {}: {e}", opts.bin)),
    };
    let registration = control::spawned(child.target());
    if let Some(s) = &opts.stdin {
        if let Some(mut si) = child.stdin.take() {
            let _ = si.write_all(s.as_bytes());
        }
    }
    // would block the child on write() while we wait, deadlocking until timeout.
    let (out_tx, out_rx) = std::sync::mpsc::channel();
    let (err_tx, err_rx) = std::sync::mpsc::channel();
    if let Some(mut s) = child.stdout.take() {
        child.add_reader(std::thread::spawn(move || {
            let buf = bounded_output(&mut s);
            let _ = out_tx.send(buf);
        }));
    }
    if let Some(mut s) = child.stderr.take() {
        child.add_reader(std::thread::spawn(move || {
            let buf = bounded_output(&mut s);
            let _ = err_tx.send(buf);
        }));
    }
    let timeout = Duration::from_millis(opts.timeout_ms.unwrap_or(30_000).max(1));
    let (status_code, timeout_error) = match child.wait_timeout(timeout) {
        Ok(Some(status)) => (status.code().unwrap_or(-1), None),
        Ok(None) => {
            child.terminate();
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
    if let Err(error) = child.finish() {
        return err(&format!("reap {}: {error}", opts.bin));
    }
    let (out, stdout_truncated) = out_rx.recv().unwrap_or_default();
    let (errout, stderr_truncated) = err_rx.recv().unwrap_or_default();
    drop(registration);
    serde_json::to_string(&ExecResult {
        code: Some(status_code),
        stdout: Some(out),
        stderr: Some(errout),
        error: timeout_error,
        truncated: (stdout_truncated || stderr_truncated).then_some(true),
        ..Default::default()
    })
    .unwrap_or_else(|_| "null".into())
}

fn run_conversation(opts: &ExecOpts, conversation: &[ExecConversationStep]) -> String {
    if opts.detach || opts.stdin.is_some() || conversation.is_empty() {
        return err("exec conversation requires stages, piped stdin, and a foreground process");
    }
    if conversation
        .iter()
        .any(|stage| stage.await_response_ids.is_empty())
    {
        return err("exec conversation stage has no awaited response ids");
    }

    let resolved = resolve_bin(&opts.bin, &well_known_bin_dirs());
    let mut command = match crate::provider_command::command(&resolved) {
        Ok(command) => command,
        Err(error) => return err(&error),
    };
    let mut child = match child::Owned::spawn(
        command
            .args(&opts.args)
            .envs(&opts.env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    ) {
        Ok(child) => child,
        Err(error) => return err(&format!("spawn {}: {error}", opts.bin)),
    };

    let registration = control::spawned(child.target());
    let Some(stdout_pipe) = child.stdout.take() else {
        return err("exec conversation stdout pipe unavailable");
    };
    let Some(stderr_pipe) = child.stderr.take() else {
        return err("exec conversation stderr pipe unavailable");
    };
    let Some(mut stdin_pipe) = child.stdin.take() else {
        return err("exec conversation stdin pipe unavailable");
    };

    let (line_tx, line_rx) = std::sync::mpsc::channel::<String>();
    child.add_reader(std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout_pipe);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if line_tx.send(line.clone()).is_err() {
                        break;
                    }
                }
            }
        }
    }));
    let (err_tx, err_rx) = std::sync::mpsc::channel::<String>();
    child.add_reader(std::thread::spawn(move || {
        let mut text = String::new();
        let mut reader = BufReader::new(stderr_pipe);
        let _ = reader.read_to_string(&mut text);
        let _ = err_tx.send(text);
    }));

    let timeout = Duration::from_millis(opts.timeout_ms.unwrap_or(30_000).max(1));
    let deadline = std::time::Instant::now() + timeout;
    let mut stdout = String::new();
    let mut failure = None;

    for stage in conversation {
        if let Err(error) = stdin_pipe
            .write_all(stage.stdin.as_bytes())
            .and_then(|_| stdin_pipe.flush())
        {
            failure = Some(format!("write {} conversation stdin: {error}", opts.bin));
            break;
        }

        let mut pending = stage
            .await_response_ids
            .iter()
            .copied()
            .collect::<HashSet<_>>();
        while !pending.is_empty() {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                failure = Some(format!(
                    "{} timed out waiting for app-server response id(s) {:?}",
                    opts.bin, pending
                ));
                break;
            }
            match line_rx.recv_timeout(remaining) {
                Ok(line) => {
                    if let Some(id) = json_rpc_response_id(&line) {
                        pending.remove(&id);
                    }
                    stdout.push_str(&line);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                    failure = Some(format!(
                        "{} timed out waiting for app-server response id(s) {:?}",
                        opts.bin, pending
                    ));
                    break;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    failure = Some(format!(
                        "{} exited before app-server response id(s) {:?}",
                        opts.bin, pending
                    ));
                    break;
                }
            }
        }
        if failure.is_some() {
            break;
        }
    }

    drop(stdin_pipe);
    if failure.is_some() {
        child.terminate();
    }

    let (status_code, wait_failure) =
        match child.wait_timeout(deadline.saturating_duration_since(std::time::Instant::now())) {
            Ok(Some(status)) => (status.code().unwrap_or(-1), None),
            Ok(None) => {
                child.terminate();
                let status = child.wait().ok();
                (
                    status.and_then(|status| status.code()).unwrap_or(-1),
                    Some(format!(
                        "{} timed out after {}ms",
                        opts.bin,
                        timeout.as_millis()
                    )),
                )
            }
            Err(error) => (-1, Some(format!("wait {}: {error}", opts.bin))),
        };

    if let Err(error) = child.finish() {
        return err(&format!("reap {}: {error}", opts.bin));
    }
    for line in line_rx.try_iter() {
        stdout.push_str(&line);
    }
    let stderr = err_rx.recv().unwrap_or_default();
    drop(registration);
    serde_json::to_string(&ExecResult {
        code: Some(status_code),
        stdout: Some(stdout),
        stderr: Some(stderr),
        error: failure.or(wait_failure),
        ..Default::default()
    })
    .unwrap_or_else(|_| "null".into())
}

fn json_rpc_response_id(line: &str) -> Option<u64> {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()?
        .get("id")?
        .as_u64()
}

fn bounded_output(reader: &mut impl Read) -> (String, bool) {
    const LIMIT: usize = 128 * 1024;
    let mut kept = Vec::new();
    let mut truncated = false;
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(count) => {
                let retain = count.min(LIMIT.saturating_sub(kept.len()));
                truncated |= retain < count;
                kept.extend_from_slice(&chunk[..retain]);
            }
        }
    }
    (String::from_utf8_lossy(&kept).into_owned(), truncated)
}

/// Extract the binary name from an exec-opts JSON blob, for the permission check.
pub(super) fn bin_of(opts_json: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(opts_json)
        .ok()
        .and_then(|v| v.get("bin").and_then(|b| b.as_str()).map(String::from))
}

// ── Async exec (job-id + poll) ──────────────────────────────────────────────

/// Completion signal for one job. A waiter uses the condvar; the worker flips
/// `done` and notifies, with the call boundary applying any failure deadline.
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

    #[cfg(test)]
    fn wait_for(&self, timeout: Duration) -> bool {
        let mut done = self.done.lock();
        if !*done {
            self.cv.wait_while_for(&mut done, |done| !*done, timeout);
        }
        *done
    }
}

#[derive(Default)]
struct ExecJob {
    plugin_id: String,
    done: bool,
    finished_at: Option<Instant>,
    done_event: std::sync::Arc<DoneEvent>,
    code: Option<i32>,
    stdout: Option<String>,
    stderr: Option<String>,
    error: Option<String>,
    truncated: bool,
    result: Option<serde_json::Value>,
    process: Arc<control::Process>,
    waiters: Vec<crate::converter::ConversionCancellation>,
    /// A retained job survives its waiters' timeouts and is removed by the poll
    /// that leaves no awaiting consumer, so a slow result is computed once.
    retained: bool,
    observation_safe: bool,
    invocation: Option<String>,
    consumers: usize,
    scope: Option<String>,
    deadline: Option<Instant>,
    timeout_result: Option<serde_json::Value>,
    cancel_hook: Option<Arc<dyn Fn() + Send + Sync>>,
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
    start_with_host_env(
        plugin_id,
        opts_json,
        &HashMap::new(),
        Arc::new(PluginSandbox::unrestricted()),
    )
}

pub(super) fn start_with_host_env(
    plugin_id: &str,
    opts_json: &str,
    host_env: &HashMap<String, String>,
    sandbox: Arc<PluginSandbox>,
) -> String {
    let opts: ExecOpts = match serde_json::from_str(opts_json) {
        Ok(opts) => opts,
        Err(error) => return err(&format!("bad exec opts: {error}")),
    };
    if let Err(error) = validate_detached_log(&opts, &sandbox) {
        return err(&error);
    }

    let job_id = match start_exec_job(plugin_id) {
        Ok(job_id) => job_id,
        Err(e) => return err(&e),
    };

    let owned = opts_json.to_owned();
    let host_env = host_env.clone();
    let Some(process) = EXEC_JOBS.lock().get(&job_id).map(|job| job.process.clone()) else {
        return err("exec cancelled before spawn");
    };
    let worker_job_id = job_id.clone();
    std::thread::spawn(move || {
        let _scope = control::Scope::new(process);
        run_exec_worker(worker_job_id, owned, host_env, sandbox)
    });

    serde_json::to_string(&codeterm_wire::plugin_host::HostJobStart { job_id })
        .unwrap_or_else(|_| "null".into())
}

fn with_host_env(mut opts: ExecOpts, host_env: &HashMap<String, String>) -> ExecOpts {
    opts.env.insert("CODETERM_API_TOKEN".into(), String::new());
    opts.env.extend(host_env.clone());
    opts
}

fn validate_detached_log(opts: &ExecOpts, sandbox: &PluginSandbox) -> Result<(), String> {
    if opts.detach {
        if let Some(path) = opts.log_file.as_deref() {
            if !sandbox.can_write(path) {
                return Err(format!(
                    "detached exec log denied: {path} is outside plugin write roots"
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn poll(plugin_id: &str, job_id: &str) -> String {
    if !observation_wait_allowed(plugin_id, job_id) {
        return err("effect job unavailable during observation");
    }
    let plugin_id = normalize_plugin_id(plugin_id);
    let mut jobs = EXEC_JOBS.lock();
    let Some(job) = jobs
        .get_mut(job_id)
        .filter(|job| job.plugin_id == plugin_id)
    else {
        return serde_json::to_string(&ExecPollResult {
            done: Some(true),
            result: ExecResult {
                error: Some("job not found".into()),
                ..Default::default()
            },
        })
        .unwrap_or_else(|_| "null".into());
    };
    if !job.done {
        return serde_json::to_string(&ExecPollResult {
            done: Some(false),
            result: ExecResult::default(),
        })
        .unwrap_or_else(|_| "null".into());
    }
    if let Some(result) = job.result.clone() {
        if job.retained {
            job.consumers = job.consumers.saturating_sub(1);
            if job.consumers > 0 {
                return result.to_string();
            }
        }
        jobs.remove(job_id);
        return result.to_string();
    }

    let out = ExecPollResult {
        done: Some(true),
        result: ExecResult {
            code: job.code,
            stdout: job.stdout.clone(),
            stderr: job.stderr.clone(),
            error: job.error.clone(),
            truncated: job.truncated.then_some(true),
            ..Default::default()
        },
    };
    jobs.remove(job_id);
    serde_json::json!(out).to_string()
}

pub(super) fn close(plugin_id: &str, job_id: &str) {
    let plugin_id = normalize_plugin_id(plugin_id);
    let removed = {
        let mut jobs = EXEC_JOBS.lock();
        if jobs
            .get(job_id)
            .is_some_and(|job| job.plugin_id == plugin_id)
        {
            jobs.remove(job_id)
        } else {
            None
        }
    };
    if let Some(job) = removed {
        if let Some(cancel) = job.cancel_hook {
            cancel();
        }
        job.process.cancel();
        job.done_event.signal();
        for waiter in job.waiters {
            waiter.notify_change();
        }
    }
}

pub(super) fn drop_plugin_jobs(plugin_id: &str) {
    let plugin_id = normalize_plugin_id(plugin_id);
    let ids: Vec<_> = EXEC_JOBS
        .lock()
        .iter()
        .filter(|(_, job)| job.plugin_id == plugin_id && job.scope.is_none())
        .map(|(id, _)| id.clone())
        .collect();
    for id in ids {
        close(&plugin_id, &id);
    }
}

#[cfg(test)]
pub(super) fn wait_json_interruptible(
    plugin_id: &str,
    job_id: &str,
    cancellation: &crate::converter::ConversionCancellation,
    deadline: Instant,
) -> WaitResult {
    wait_json_until(plugin_id, job_id, cancellation, deadline, || {
        cancellation.is_cancelled()
    })
}

pub(super) fn wait_json_until(
    plugin_id: &str,
    job_id: &str,
    cancellation: &crate::converter::ConversionCancellation,
    deadline: Instant,
    cancelled: impl Fn() -> bool,
) -> WaitResult {
    if !observation_wait_allowed(plugin_id, job_id) {
        return WaitResult::Completed(err("effect job unavailable during observation"));
    }
    let plugin_id = normalize_plugin_id(plugin_id);
    let deadline = {
        let mut jobs = EXEC_JOBS.lock();
        if let Some(job) = jobs
            .get_mut(job_id)
            .filter(|job| job.plugin_id == plugin_id)
        {
            job.waiters.push(cancellation.clone());
            job.deadline
                .map_or(deadline, |job_deadline| job_deadline.min(deadline))
        } else {
            deadline
        }
    };
    loop {
        let generation = cancellation.generation();
        if cancelled() {
            abandon(&plugin_id, job_id);
            return WaitResult::TimedOut;
        }
        if Instant::now() >= deadline {
            if let Some(result) = scoped::timeout_result(&plugin_id, job_id) {
                return WaitResult::Completed(result);
            }
            abandon(&plugin_id, job_id);
            return WaitResult::TimedOut;
        }
        let done = EXEC_JOBS
            .lock()
            .get(job_id)
            .filter(|job| job.plugin_id == plugin_id)
            .is_none_or(|job| job.done);
        if done {
            return WaitResult::Completed(poll(&plugin_id, job_id));
        }
        cancellation.wait_for_change(
            generation,
            deadline.saturating_duration_since(Instant::now()),
        );
    }
}

/// Wait for `job_id` without a VM lock, or return the bounded refusal outcome.
pub(super) enum WaitResult {
    Completed(String),
    TimedOut,
}

#[cfg(test)]
pub(super) fn wait_json(plugin_id: &str, job_id: &str) -> WaitResult {
    if !observation_wait_allowed(plugin_id, job_id) {
        return WaitResult::Completed(err("effect job unavailable during observation"));
    }
    let plugin_id = normalize_plugin_id(plugin_id);
    let (event, deadline) = {
        let jobs = EXEC_JOBS.lock();
        match jobs.get(job_id).filter(|job| job.plugin_id == plugin_id) {
            Some(job) => (job.done_event.clone(), job.deadline),
            None => {
                return WaitResult::Completed(
                    serde_json::to_string(&ExecPollResult {
                        done: Some(true),
                        result: ExecResult {
                            error: Some("job not found".into()),
                            ..Default::default()
                        },
                    })
                    .unwrap_or_else(|_| "null".into()),
                )
            }
        }
    };
    let bound = deadline.map_or_else(
        || wait_bound(&plugin_id),
        |deadline| deadline.saturating_duration_since(Instant::now()),
    );
    if !event.wait_for(bound) {
        if let Some(result) = scoped::timeout_result(&plugin_id, job_id) {
            return WaitResult::Completed(result);
        }
        abandon(&plugin_id, job_id);
        return WaitResult::TimedOut;
    }
    WaitResult::Completed(poll(&plugin_id, job_id))
}

pub(super) fn bind_invocation(plugin_id: &str, job_id: &str, invocation: String) {
    let plugin_id = normalize_plugin_id(plugin_id);
    if let Some(job) = EXEC_JOBS
        .lock()
        .get_mut(job_id)
        .filter(|job| job.plugin_id == plugin_id)
    {
        job.invocation = Some(invocation);
    }
}

fn invocation_owned(plugin_id: &str, job_id: &str) -> bool {
    let plugin_id = normalize_plugin_id(plugin_id);
    let invocation = crate::surface_invocation::root_identity();
    EXEC_JOBS.lock().get(job_id).is_some_and(|job| {
        job.plugin_id == plugin_id
            && job
                .invocation
                .as_ref()
                .is_none_or(|owner| owner == &invocation)
    })
}

pub(super) fn poll_owned(plugin_id: &str, job_id: &str) -> String {
    if !invocation_owned(plugin_id, job_id) {
        return err("host job belongs to another invocation");
    }
    poll(plugin_id, job_id)
}

pub(super) fn close_owned(plugin_id: &str, job_id: &str) {
    if invocation_owned(plugin_id, job_id) {
        close(plugin_id, job_id);
    }
}

pub(super) fn allow_observation(plugin_id: &str, job_id: &str) {
    let plugin_id = normalize_plugin_id(plugin_id);
    if let Some(job) = EXEC_JOBS
        .lock()
        .get_mut(job_id)
        .filter(|job| job.plugin_id == plugin_id)
    {
        job.observation_safe = true;
    }
}

fn observation_wait_allowed(plugin_id: &str, job_id: &str) -> bool {
    if codeterm_decision::physical::effects_allowed() {
        return true;
    }
    let plugin_id = normalize_plugin_id(plugin_id);
    EXEC_JOBS
        .lock()
        .get(job_id)
        .is_some_and(|job| job.plugin_id == plugin_id && (job.retained || job.observation_safe))
}

pub(super) fn await_ticket_owned(plugin_id: &str, job_id: &str) -> bool {
    invocation_owned(plugin_id, job_id)
        && observation_wait_allowed(plugin_id, job_id)
        && EXEC_JOBS
            .lock()
            .get(job_id)
            .is_some_and(|job| job.plugin_id == plugin_id)
}

/// A waiter that gives up leaves a retained job running for the next consumer.
fn abandon(plugin_id: &str, job_id: &str) {
    {
        let mut jobs = EXEC_JOBS.lock();
        if let Some(job) = jobs
            .get_mut(job_id)
            .filter(|job| job.plugin_id == plugin_id && job.retained)
        {
            job.consumers = job.consumers.saturating_sub(1);
            return;
        }
    }
    close(plugin_id, job_id);
}

#[cfg(not(test))]
pub(super) fn wait_bound(_plugin_id: &str) -> Duration {
    ASYNC_JOB_WAIT_BOUND
}

#[cfg(test)]
static TEST_WAIT_BOUNDS: LazyLock<Mutex<HashMap<String, Duration>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[cfg(test)]
pub(super) fn wait_bound(plugin_id: &str) -> Duration {
    TEST_WAIT_BOUNDS
        .lock()
        .get(plugin_id)
        .copied()
        .unwrap_or(ASYNC_JOB_WAIT_BOUND)
}

#[cfg(test)]
pub(super) fn set_test_wait_bound(plugin_id: &str, bound: Duration) {
    TEST_WAIT_BOUNDS.lock().insert(plugin_id.to_string(), bound);
}

#[cfg(test)]
pub(super) fn plugin_job_count(plugin_id: &str) -> usize {
    let plugin_id = normalize_plugin_id(plugin_id);
    EXEC_JOBS
        .lock()
        .values()
        .filter(|job| job.plugin_id == plugin_id)
        .count()
}

/// A job that outlives a timed-out waiter; `attach_retained` joins it.
pub(super) fn start_retained_json_job<F>(plugin_id: &str, work: F) -> Result<String, String>
where
    F: FnOnce() -> String + Send + 'static,
{
    let job_id = start_exec_job(plugin_id)?;
    if let Some(job) = EXEC_JOBS.lock().get_mut(&job_id) {
        job.retained = true;
        job.consumers = 1;
    }
    spawn_json_worker(job_id.clone(), work);
    Ok(job_id)
}

/// Adds one consumer to a live retained job; `false` once it is gone.
pub(super) fn attach_retained(plugin_id: &str, job_id: &str) -> bool {
    let plugin_id = normalize_plugin_id(plugin_id);
    let mut jobs = EXEC_JOBS.lock();
    match jobs
        .get_mut(job_id)
        .filter(|job| job.plugin_id == plugin_id && job.retained)
    {
        Some(job) => {
            job.consumers += 1;
            true
        }
        None => false,
    }
}

pub(super) fn retained_job_live(plugin_id: &str, job_id: &str) -> bool {
    let plugin_id = normalize_plugin_id(plugin_id);
    EXEC_JOBS
        .lock()
        .get(job_id)
        .is_some_and(|job| job.plugin_id == plugin_id && job.retained)
}

fn spawn_json_worker<F>(job_id: String, work: F)
where
    F: FnOnce() -> String + Send + 'static,
{
    let failed_job = job_id.clone();
    let spawned = std::thread::Builder::new()
        .name("plugin-host-job".into())
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
                .unwrap_or_else(|_| {
                    serde_json::json!({ "error": "host job panicked" }).to_string()
                });
            finish_json_job(&job_id, &result);
        });
    if spawned.is_err() {
        finish_json_job(&failed_job, r#"{"error":"host job worker unavailable"}"#);
    }
}

pub(super) fn start_json_job<F>(plugin_id: &str, work: F) -> Result<String, String>
where
    F: FnOnce() -> String + Send + 'static,
{
    let job_id = start_exec_job(plugin_id)?;
    spawn_json_worker(job_id.clone(), work);
    Ok(job_id)
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
    reap_expired_jobs(&mut jobs, Instant::now());
    let plugin_jobs = jobs
        .values()
        .filter(|job| job.plugin_id == plugin_id)
        .count();
    if plugin_jobs >= MAX_EXEC_JOBS_PER_PLUGIN {
        return Err(format!(
            "plugin {plugin_id} reached the exec job limit ({MAX_EXEC_JOBS_PER_PLUGIN})"
        ));
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

fn reap_expired_jobs(jobs: &mut HashMap<String, ExecJob>, now: Instant) {
    jobs.retain(|_, job| {
        let age = job
            .finished_at
            .and_then(|finished_at| now.checked_duration_since(finished_at));
        !matches!(age, Some(age) if age >= EXEC_JOB_RETENTION)
    });
}

fn run_exec_worker(
    job_id: String,
    opts_json: String,
    host_env: HashMap<String, String>,
    sandbox: Arc<PluginSandbox>,
) {
    let value = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_with_host_env(&opts_json, &host_env, &sandbox)
    })) {
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
        job.finished_at = Some(Instant::now());
        job.truncated = value.get("truncated").and_then(|v| v.as_bool()) == Some(true);
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
        for waiter in &job.waiters {
            waiter.notify_change();
        }
    }
}

fn finish_json_job(job_id: &str, result: &str) {
    let result = serde_json::from_str::<serde_json::Value>(result)
        .unwrap_or_else(|_| serde_json::json!({ "error": "host job returned invalid json" }));
    let mut jobs = EXEC_JOBS.lock();
    if let Some(job) = jobs.get_mut(job_id) {
        job.done = true;
        job.finished_at = Some(Instant::now());
        job.result = Some(result);
        job.done_event.signal();
        for waiter in &job.waiters {
            waiter.notify_change();
        }
    }
}

#[cfg(test)]
mod tests;

pub(super) fn export_protocol_contracts(
    config: &ts_rs::Config,
) -> Result<(), Box<dyn std::error::Error>> {
    use ts_rs::TS;
    protocol::export(config)?;
    ExecOpts::export_all(config)?;
    ExecConversationStep::export_all(config)?;
    Ok(())
}
