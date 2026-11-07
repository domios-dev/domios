#[cfg(windows)]
pub(super) mod windows;

use super::fs::MAX_BYTES;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
    process::Stdio,
    sync::Arc,
};
use tokio::{io::AsyncReadExt, sync::watch};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Command {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub env: Vec<Env>,
    #[serde(default)]
    pub output_byte_limit: Option<usize>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Env {
    pub name: String,
    pub value: String,
}

pub(super) fn environment(
    base: HashMap<String, String>,
    changes: &[Env],
    exclusions: &[String],
) -> Result<BTreeMap<String, String>, String> {
    let launch = base
        .iter()
        .filter(|(key, _)| {
            codeterm_util::tab_launch::MARKERS.contains(&key.as_str())
                || [
                    "CODETERM_API_URL",
                    "CODETERM_API_PORT",
                    "CODETERM_SOCKET_PATH",
                    "CODETERM_CLI_PATH",
                    "CODETERM_TEAM_ID",
                    "CODETERM_PLAN_ID",
                    "CODETERM_LANE_BASE",
                ]
                .contains(&key.as_str())
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<BTreeMap<_, _>>();
    let allowed = |key: &str| {
        [
            "PATH",
            "LANG",
            "LC_ALL",
            "LC_CTYPE",
            "TERM",
            "TMPDIR",
            "TMP",
            "TEMP",
            "SYSTEMROOT",
            "WINDIR",
            "PATHEXT",
        ]
        .contains(&key.to_ascii_uppercase().as_str())
            && !exclusions
                .iter()
                .any(|excluded| excluded.eq_ignore_ascii_case(key))
    };
    let mut env: BTreeMap<_, _> = std::env::vars_os()
        .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
        .chain(base)
        .filter(|(key, _)| allowed(key))
        .collect();
    if changes.len() > 32 {
        return Err("Too many environment changes".into());
    }
    for entry in changes {
        if !allowed(&entry.name) || entry.value.len() > 4096 || entry.value.contains('\0') {
            return Err("Environment change denied".into());
        }
        #[cfg(windows)]
        {
            if entry.name.eq_ignore_ascii_case("PATH") {
                windows::validate_agent_path(&entry.value)?;
            }
            env.retain(|key, _| !key.eq_ignore_ascii_case(&entry.name));
        }
        env.insert(entry.name.clone(), entry.value.clone());
    }
    env.extend(launch);
    Ok(env)
}

pub(super) struct Terminal {
    pub output: Arc<Mutex<Vec<u8>>>,
    pub truncated: Arc<Mutex<bool>>,
    stop: watch::Sender<bool>,
    ended: watch::Receiver<Option<Result<Value, String>>>,
    limit: usize,
    #[cfg(test)]
    pub changes: watch::Receiver<()>,
}
impl Drop for Terminal {
    fn drop(&mut self) {
        self.stop.send_replace(true);
    }
}
impl Terminal {
    pub fn spawn(
        spec: &Command,
        _cwd: &Path,
        directory: std::fs::File,
        env: &BTreeMap<String, String>,
    ) -> Result<Arc<Self>, String> {
        #[cfg(windows)]
        let resolved_program = windows::program(
            &spec.command,
            env,
            spec.env
                .iter()
                .any(|entry| entry.name.eq_ignore_ascii_case("PATH")),
        )?;
        #[cfg(windows)]
        let program = &resolved_program.path;
        #[cfg(not(windows))]
        let program = &spec.command;
        let mut command = tokio::process::Command::new(program);
        command
            .args(&spec.args)
            .current_dir("/")
            .env_clear()
            .envs(env)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(windows)]
        let pinned_cwd = super::fs::windows::pinned(_cwd)?;
        #[cfg(windows)]
        super::fs::windows::same(pinned_cwd.last().ok_or("Missing terminal cwd")?, &directory)?;
        #[cfg(windows)]
        command.current_dir(super::fs::windows::cwd(&directory)?);
        #[cfg(windows)]
        crate::process_util::no_window_tokio_suspended(&mut command);
        #[cfg(unix)]
        let lease = super::super::lease::Lease::configure(&mut command)
            .map_err(|error| error.to_string())?;
        #[cfg(unix)]
        unsafe {
            command.pre_exec(move || {
                use std::os::fd::AsRawFd;
                if libc::fchdir(directory.as_raw_fd()) == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            });
        }
        let mut child = command.spawn().map_err(|error| error.to_string())?;
        #[cfg(windows)]
        drop((resolved_program, pinned_cwd, directory));
        let pid = child.id().ok_or("Terminal PID unavailable")?;
        #[cfg(windows)]
        let job = Arc::new(crate::service::windows_job::Job::contain(&child)?);
        let boundary = Arc::new(
            super::super::process_boundary::Boundary::new(
                pid,
                #[cfg(windows)]
                job,
            )
            .map_err(|error| error.to_string())?,
        );
        #[cfg(windows)]
        crate::service::windows_job::resume(&child)?;
        let mut stdout = child.stdout.take().ok_or("Missing terminal stdout")?;
        let mut stderr = child.stderr.take().ok_or("Missing terminal stderr")?;
        let output = Arc::new(Mutex::new(Vec::new()));
        let truncated = Arc::new(Mutex::new(false));
        let limit = spec.output_byte_limit.unwrap_or(MAX_BYTES).min(MAX_BYTES);
        let out = output.clone();
        let clipped = truncated.clone();
        #[cfg(test)]
        let (changed, changes) = watch::channel(());
        let reader = crate::async_runtime::spawn(async move {
            let mut a = [0; 4096];
            let mut b = [0; 4096];
            let mut stdout_open = true;
            let mut stderr_open = true;
            while stdout_open || stderr_open {
                let result = tokio::select! {
                    result = stdout.read(&mut a), if stdout_open => (true,result),
                    result = stderr.read(&mut b), if stderr_open => (false,result),
                };
                match result {
                    (true, Ok(0) | Err(_)) => stdout_open = false,
                    (false, Ok(0) | Err(_)) => stderr_open = false,
                    (is_stdout, Ok(count)) => {
                        let bytes = if is_stdout { &a[..count] } else { &b[..count] };
                        let mut held = out.lock();
                        held.extend_from_slice(bytes);
                        if held.len() > limit {
                            let excess = held.len() - limit;
                            held.drain(..excess);
                            while held.first().is_some_and(|byte| byte & 0xc0 == 0x80) {
                                held.remove(0);
                            }
                            *clipped.lock() = true;
                        }
                        #[cfg(test)]
                        changed.send_replace(());
                    }
                }
            }
        });
        let (stop, mut stopping) = watch::channel(false);
        let (finished, ended) = watch::channel(None);
        #[cfg(unix)]
        let mut witnessed = tokio::task::spawn_blocking(move || {
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            loop {
                if unsafe {
                    libc::waitid(libc::P_PID, pid, &mut info, libc::WEXITED | libc::WNOWAIT)
                } == 0
                {
                    return Ok::<_, std::io::Error>(());
                }
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::EINTR) {
                    return Err(error);
                }
            }
        });
        crate::async_runtime::spawn(async move {
            #[cfg(unix)]
            tokio::select! { _ = stopping.changed() => {}, _ = &mut witnessed => {} }
            #[cfg(windows)]
            tokio::select! { _ = stopping.changed() => {}, _ = child.wait() => {} }
            let retiring = boundary.clone();
            let retired = tokio::task::spawn_blocking(move || retiring.retire()).await;
            let _ = boundary.force_terminate();
            let status = child.wait().await;
            #[cfg(unix)]
            drop(lease);
            let _ = reader.await;
            let result = (|| {
                retired
                    .map_err(|error| error.to_string())?
                    .map_err(|error| error.to_string())?;
                let status = status.map_err(|error| error.to_string())?;
                #[cfg(unix)]
                let signal = {
                    use std::os::unix::process::ExitStatusExt;
                    status.signal().map(|signal| signal.to_string())
                };
                #[cfg(not(unix))]
                let signal: Option<String> = None;
                Ok(json!({"exitCode":status.code(),"signal":signal}))
            })();
            finished.send_replace(Some(result));
        });
        Ok(Arc::new(Self {
            output,
            truncated,
            stop,
            ended,
            limit,
            #[cfg(test)]
            changes,
        }))
    }
    pub fn output(&self) -> Value {
        let mut text = String::from_utf8_lossy(&self.output.lock()).into_owned();
        if text.len() > self.limit {
            let mut start = text.len() - self.limit;
            while !text.is_char_boundary(start) {
                start += 1;
            }
            text.drain(..start);
        }
        json!({"output":text,"truncated":*self.truncated.lock(),"exitStatus":self.ended.borrow().as_ref().and_then(|result| result.as_ref().ok()).cloned()})
    }
    pub async fn wait(&self) -> Result<Value, String> {
        let mut ended = self.ended.clone();
        loop {
            if let Some(result) = ended.borrow_and_update().clone() {
                return result;
            }
            ended
                .changed()
                .await
                .map_err(|_| "Terminal exit evidence unavailable")?;
        }
    }
    pub async fn kill(&self) -> Result<Value, String> {
        self.stop.send_replace(true);
        self.wait().await
    }
}
