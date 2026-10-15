#[cfg(any(windows, test))]
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[path = "process.rs"]
pub(crate) mod process;
pub(super) use process::owner_alive;

pub(super) struct UpdateCommandFailure {
    pub(super) reason: String,
    pub(super) child_stopped: bool,
}

impl UpdateCommandFailure {
    fn before_spawn(stage: FailureStage, reason: String) -> Self {
        Self {
            reason: format!("{stage:?}: {reason}"),
            child_stopped: true,
        }
    }
}

pub(super) fn run_update_command(command: &str) -> Result<(), UpdateCommandFailure> {
    let (executable, args) = update_argv(command)
        .map_err(|error| UpdateCommandFailure::before_spawn(FailureStage::Resolve, error))?;
    run_invocation(Invocation {
        executable,
        args,
        cwd: None,
        environment: EnvironmentPolicy::Inherited,
        execution: std::time::Duration::from_secs(300),
        cleanup: std::time::Duration::from_secs(5),
        #[cfg(test)]
        controls: ExecutorControls::default(),
    })
}

enum EnvironmentPolicy {
    Inherited,
    #[cfg(test)]
    Explicit(Vec<(std::ffi::OsString, std::ffi::OsString)>),
}

struct Invocation {
    executable: PathBuf,
    args: Vec<std::ffi::OsString>,
    cwd: Option<PathBuf>,
    environment: EnvironmentPolicy,
    execution: std::time::Duration,
    cleanup: std::time::Duration,
    #[cfg(test)]
    controls: ExecutorControls,
}

#[cfg(test)]
#[derive(Default)]
struct ExecutorControls {
    deadline_gate: Option<std::sync::mpsc::Receiver<()>>,
    capture_setup_error: bool,
    capture_read_error: bool,
    capture_disconnect: bool,
    termination_error: bool,
    parent_event: Option<std::sync::mpsc::Sender<()>>,
    parent_continue: Option<std::sync::mpsc::Receiver<()>>,
    expire_capture: bool,
}

impl Invocation {
    fn command(&self) -> Command {
        let mut command = Command::new(&self.executable);
        codeterm_util::process_util::no_window_std(&mut command);
        command.args(&self.args);
        if let Some(cwd) = &self.cwd {
            command.current_dir(cwd);
        }
        match &self.environment {
            EnvironmentPolicy::Inherited => {}
            #[cfg(test)]
            EnvironmentPolicy::Explicit(values) => {
                command.env_clear().envs(values.iter().cloned());
            }
        }
        command
    }
}

#[derive(Debug, Clone, Copy)]
enum FailureStage {
    Resolve,
    Prepare,
    Spawn,
    SpawnOwnership,
    CaptureSetup,
    Attach,
    ParentWait,
    ParentExit,
    ExecutionDeadline,
    Terminate,
    CleanupWait,
    Boundary,
    CaptureRead,
    CaptureDisconnect,
    CaptureDeadline,
}

#[derive(Debug)]
struct ExecutionError {
    stage: FailureStage,
    detail: String,
}

#[derive(Default)]
struct Completion {
    parent: Option<std::process::ExitStatus>,
    boundary: Option<process::BoundaryCompletion>,
    errors: Vec<ExecutionError>,
}

impl Completion {
    fn fail(&mut self, stage: FailureStage, detail: impl ToString) {
        self.errors.push(ExecutionError {
            stage,
            detail: detail.to_string().chars().take(2048).collect(),
        });
    }

    fn observe(&mut self, tree: &process::ProcessTree) {
        match tree.observe_event() {
            Ok(boundary) => self.boundary = Some(boundary),
            Err(error) => {
                self.boundary = None;
                self.fail(FailureStage::Boundary, error);
            }
        }
    }

    fn stopped(&self, streams: &[Option<Capture>; 2]) -> bool {
        self.parent.is_some()
            && self
                .boundary
                .is_some_and(process::BoundaryCompletion::confirmed)
            && streams
                .iter()
                .all(|stream| stream.as_ref().is_some_and(Capture::resources_complete))
    }

    fn finish(mut self, streams: &[Option<Capture>; 2]) -> Result<(), UpdateCommandFailure> {
        let stopped = self.stopped(streams);
        let clean = streams
            .iter()
            .all(|stream| stream.as_ref().is_some_and(Capture::clean_eof));
        if self.errors.is_empty()
            && self.parent.is_some_and(|status| status.success())
            && stopped
            && clean
        {
            return Ok(());
        }
        if self.errors.is_empty() {
            self.fail(
                FailureStage::Boundary,
                "owned updater completion unconfirmed",
            );
        }
        let reasons = self
            .errors
            .iter()
            .map(|error| format!("{:?}: {}", error.stage, error.detail))
            .collect::<Vec<_>>()
            .join("\n");
        Err(UpdateCommandFailure {
            reason: format!(
                "{reasons}\nstdout:\n{}\nstderr:\n{}{}",
                captured(streams[0].as_ref()),
                captured(streams[1].as_ref()),
                if stopped {
                    ""
                } else {
                    "\nupdater termination unconfirmed; further updates are withheld"
                }
            ),
            child_stopped: stopped,
        })
    }
}

fn run_invocation(invocation: Invocation) -> Result<(), UpdateCommandFailure> {
    use std::time::Instant;
    use wait_timeout::ChildExt;
    let execution_deadline = Instant::now() + invocation.execution;
    let mut command = invocation.command();
    let mut tree = process::ProcessTree::prepare(&mut command)
        .map_err(|error| UpdateCommandFailure::before_spawn(FailureStage::Prepare, error))?;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            UpdateCommandFailure::before_spawn(FailureStage::Spawn, error.to_string())
        })?;
    #[cfg(test)]
    tree.inject_termination_error(invocation.controls.termination_error);
    let mut completion = Completion::default();
    let mut streams = [None, None];
    if let Err(error) = tree.record_spawn(&child) {
        completion.fail(FailureStage::SpawnOwnership, error);
    }
    for (index, stream) in [
        child
            .stdout
            .take()
            .map(|stream| Box::new(stream) as Box<dyn std::io::Read + Send>),
        child
            .stderr
            .take()
            .map(|stream| Box::new(stream) as Box<dyn std::io::Read + Send>),
    ]
    .into_iter()
    .enumerate()
    {
        let capture = stream
            .ok_or_else(|| "capture stream unavailable".to_string())
            .and_then(|stream| {
                #[cfg(test)]
                if index == 0 {
                    if invocation.controls.capture_setup_error {
                        return Err("injected capture setup failure".into());
                    }
                    if invocation.controls.capture_read_error {
                        return capture(InjectedReadFailure(Some(stream)));
                    }
                    if invocation.controls.capture_disconnect {
                        drop(stream);
                        let (tx, receiver) = std::sync::mpsc::channel();
                        drop(tx);
                        return Ok(Capture {
                            receiver,
                            tail: Default::default(),
                            ended: None,
                        });
                    }
                }
                capture(stream)
            });
        match capture {
            Ok(capture) => streams[index] = Some(capture),
            Err(error) => completion.fail(FailureStage::CaptureSetup, error),
        }
    }
    if completion.errors.is_empty() {
        if let Err(error) = tree.attach(&child) {
            completion.fail(FailureStage::Attach, error);
        }
    }
    #[cfg(test)]
    let execution_deadline = if let Some(gate) = &invocation.controls.deadline_gate {
        match gate.recv_timeout(invocation.execution) {
            Ok(()) => Instant::now(),
            Err(error) => {
                completion.fail(FailureStage::ExecutionDeadline, error);
                execution_deadline
            }
        }
    } else {
        execution_deadline
    };
    if completion.errors.is_empty() && Instant::now() >= execution_deadline {
        completion.fail(
            FailureStage::ExecutionDeadline,
            "provider update exceeded its execution deadline",
        );
    }
    if completion.errors.is_empty() {
        match child.wait_timeout(execution_deadline.saturating_duration_since(Instant::now())) {
            Ok(Some(status)) => {
                tree.reaped(&child);
                completion.parent = Some(status);
                completion.observe(&tree);
                if Instant::now() >= execution_deadline {
                    completion.fail(
                        FailureStage::ExecutionDeadline,
                        "provider update parent exit was observed after its execution deadline",
                    );
                }
                if !status.success() {
                    completion.fail(
                        FailureStage::ParentExit,
                        format!("provider update exited with {status}"),
                    );
                }
            }
            Ok(None) => completion.fail(
                FailureStage::ExecutionDeadline,
                "provider update exceeded its execution deadline",
            ),
            Err(error) => {
                tree.wait_failed();
                completion.fail(FailureStage::ParentWait, error);
            }
        }
    }
    #[cfg(test)]
    let execution_deadline = if completion.parent.is_some() {
        if let Some(event) = &invocation.controls.parent_event {
            let _ = event.send(());
        }
        if let Some(gate) = &invocation.controls.parent_continue {
            if let Err(error) = gate.recv_timeout(invocation.execution) {
                completion.fail(FailureStage::ExecutionDeadline, error);
            }
        }
        if invocation.controls.expire_capture {
            Instant::now()
        } else {
            execution_deadline
        }
    } else {
        execution_deadline
    };
    if completion.errors.is_empty() {
        receive_streams(
            &mut streams,
            execution_deadline,
            &mut completion,
            &tree,
            true,
        );
    }
    #[cfg(windows)]
    if completion.errors.is_empty()
        && !completion
            .boundary
            .is_some_and(process::BoundaryCompletion::confirmed)
    {
        match tree.wait_completion(execution_deadline) {
            Ok(boundary) => completion.boundary = Some(boundary),
            Err(error) => completion.fail(FailureStage::Boundary, error),
        }
    }
    if completion.errors.is_empty()
        && !completion
            .boundary
            .is_some_and(process::BoundaryCompletion::confirmed)
    {
        completion.fail(FailureStage::Boundary, "original visible updater group remains present or unconfirmed after parent/capture events");
    }
    if !completion.errors.is_empty() {
        let cleanup_deadline = Instant::now() + invocation.cleanup;
        if !completion
            .boundary
            .is_some_and(process::BoundaryCompletion::confirmed)
        {
            if let Err(error) = tree.terminate(&mut child) {
                completion.fail(FailureStage::Terminate, error);
            }
        }
        if completion.parent.is_none() {
            match child.wait_timeout(cleanup_deadline.saturating_duration_since(Instant::now())) {
                Ok(Some(status)) => {
                    tree.reaped(&child);
                    completion.parent = Some(status);
                    completion.observe(&tree);
                }
                Ok(None) => completion.fail(
                    FailureStage::CleanupWait,
                    "updater parent did not exit within the shared cleanup budget",
                ),
                Err(error) => {
                    tree.wait_failed();
                    completion.fail(FailureStage::CleanupWait, error);
                }
            }
        }
        receive_streams(
            &mut streams,
            cleanup_deadline,
            &mut completion,
            &tree,
            false,
        );
        #[cfg(windows)]
        if !completion
            .boundary
            .is_some_and(process::BoundaryCompletion::confirmed)
        {
            match tree.wait_completion(cleanup_deadline) {
                Ok(boundary) => completion.boundary = Some(boundary),
                Err(error) => completion.fail(FailureStage::Boundary, error),
            }
        }
    }
    #[cfg(windows)]
    if let Err(error) = tree.close_handles() {
        completion.fail(FailureStage::Terminate, error);
    }
    completion.finish(&streams)
}

fn receive_streams(
    streams: &mut [Option<Capture>; 2],
    deadline: std::time::Instant,
    completion: &mut Completion,
    tree: &process::ProcessTree,
    stop_on_error: bool,
) {
    for stream in streams.iter_mut().flatten() {
        if stream.resources_complete() {
            continue;
        }
        match stream.receive(deadline) {
            Ok(()) => completion.observe(tree),
            Err(error) => {
                if stream.resources_complete() {
                    completion.observe(tree);
                }
                completion.fail(error.stage, error.detail);
                if stop_on_error {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
struct InjectedReadFailure(Option<Box<dyn std::io::Read + Send>>);

#[cfg(test)]
impl std::io::Read for InjectedReadFailure {
    fn read(&mut self, chunk: &mut [u8]) -> std::io::Result<usize> {
        match self.0.take() {
            Some(mut stream) => stream.read(chunk),
            None => Err(std::io::Error::other("injected capture read failure")),
        }
    }
}

struct Capture {
    receiver: std::sync::mpsc::Receiver<Result<(), std::io::Error>>,
    tail: std::sync::Arc<parking_lot::Mutex<std::collections::VecDeque<u8>>>,
    ended: Option<bool>,
}

impl Capture {
    fn resources_complete(&self) -> bool {
        self.ended.is_some()
    }
    fn clean_eof(&self) -> bool {
        self.ended == Some(true)
    }
    fn receive(&mut self, deadline: std::time::Instant) -> Result<(), ExecutionError> {
        match self
            .receiver
            .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
        {
            Ok(Ok(())) => {
                self.ended = Some(true);
                if std::time::Instant::now() >= deadline {
                    Err(ExecutionError {
                        stage: FailureStage::CaptureDeadline,
                        detail: "capture completion was observed after its failure deadline".into(),
                    })
                } else {
                    Ok(())
                }
            }
            Ok(Err(error)) => {
                self.ended = Some(false);
                Err(ExecutionError {
                    stage: FailureStage::CaptureRead,
                    detail: error.to_string(),
                })
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(ExecutionError {
                stage: FailureStage::CaptureDisconnect,
                detail: "capture worker disconnected without a resource-completion receipt".into(),
            }),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(ExecutionError {
                stage: FailureStage::CaptureDeadline,
                detail: "capture stream did not complete within its failure deadline".into(),
            }),
        }
    }
}

fn capture(mut stream: impl std::io::Read + Send + 'static) -> Result<Capture, String> {
    let (tx, receiver) = std::sync::mpsc::channel();
    let tail = std::sync::Arc::new(parking_lot::Mutex::new(
        std::collections::VecDeque::<u8>::with_capacity(32 * 1024),
    ));
    let output = tail.clone();
    std::thread::Builder::new()
        .name("provider-update-capture".into())
        .spawn(move || {
            let mut chunk = [0u8; 4096];
            let result = loop {
                match stream.read(&mut chunk) {
                    Ok(0) => break Ok(()),
                    Ok(size) => {
                        let mut tail = output.lock();
                        let excess = (tail.len() + size).saturating_sub(32 * 1024);
                        tail.drain(..excess);
                        tail.extend(&chunk[..size]);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) => break Err(error),
                }
            };
            drop(stream);
            let _ = tx.send(result);
        })
        .map_err(|error| error.to_string())?;
    Ok(Capture {
        receiver,
        tail,
        ended: None,
    })
}

fn captured(capture: Option<&Capture>) -> String {
    capture
        .map(|capture| {
            String::from_utf8_lossy(&capture.tail.lock().iter().copied().collect::<Vec<_>>())
                .into_owned()
        })
        .unwrap_or_else(|| "capture was not established".into())
}

enum ResolutionPolicy {
    Production,
    #[cfg(test)]
    Confined(codeterm_plugin::js_runtime::BinSearchEnvironment),
}

impl ResolutionPolicy {
    fn resolve(&self, executable: &str) -> codeterm_plugin::js_runtime::BinResolution {
        match self {
            Self::Production => codeterm_plugin::js_runtime::probe_bin(executable).resolution,
            #[cfg(test)]
            Self::Confined(environment) => {
                codeterm_plugin::js_runtime::probe_bin_with_environment(
                    executable,
                    &[],
                    environment,
                )
                .resolution
            }
        }
    }
}

fn update_argv(command: &str) -> Result<(PathBuf, Vec<std::ffi::OsString>), String> {
    update_argv_with_policy(command, &ResolutionPolicy::Production)
}

fn update_argv_with_policy(
    command: &str,
    policy: &ResolutionPolicy,
) -> Result<(PathBuf, Vec<std::ffi::OsString>), String> {
    let mut parts = command.split_whitespace();
    let executable = parts
        .next()
        .ok_or_else(|| "empty provider update command".to_string())?;
    if !executable
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || "_./\\:-".contains(ch))
    {
        return Err("provider update executable contains unsupported characters".into());
    }
    let args = parts.collect::<Vec<_>>();
    if args.iter().any(|arg| {
        !arg.chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "_./\\:-".contains(ch))
    }) {
        return Err("provider update arguments contain unsupported characters".into());
    }
    let resolved = match policy.resolve(executable) {
        codeterm_plugin::js_runtime::BinResolution::Found(path) => path,
        codeterm_plugin::js_runtime::BinResolution::Missing
        | codeterm_plugin::js_runtime::BinResolution::Unknown => {
            #[cfg(test)]
            if matches!(policy, ResolutionPolicy::Confined(_)) {
                return Err(format!(
                    "fixture executable {executable} is absent from its confined resolution roots"
                ));
            }
            #[cfg(windows)]
            return Err(format!(
                "provider update executable {executable} was not resolved through PATH/PATHEXT"
            ));
            #[cfg(not(windows))]
            PathBuf::from(executable)
        }
    };
    #[cfg(windows)]
    if resolved
        .extension()
        .is_some_and(|extension| extension.to_string_lossy().eq_ignore_ascii_case("cmd"))
    {
        return windows_npm_shim_argv(&resolved, &args, policy);
    }
    Ok((resolved, args.into_iter().map(Into::into).collect()))
}

#[cfg(any(windows, test))]
fn windows_npm_shim_argv(
    shim_path: &Path,
    args: &[&str],
    policy: &ResolutionPolicy,
) -> Result<(PathBuf, Vec<std::ffi::OsString>), String> {
    let contents = std::fs::read_to_string(shim_path).map_err(|error| error.to_string())?;
    let script = npm_shim_script(shim_path.parent().unwrap_or(Path::new(".")), &contents)
        .ok_or_else(|| {
            format!(
                "could not resolve the node entrypoint in {}",
                shim_path.display()
            )
        })?;
    let node = match policy.resolve("node") {
        codeterm_plugin::js_runtime::BinResolution::Found(path) => path,
        _ => return Err("Node.js was not found while resolving the provider Windows shim".into()),
    };
    let mut argv = vec![script.into_os_string()];
    argv.extend(args.iter().map(std::ffi::OsString::from));
    Ok((node, argv))
}

#[cfg(any(windows, test))]
pub(super) fn npm_shim_script(parent: &Path, contents: &str) -> Option<PathBuf> {
    let marker = "\"%_prog%\" \"";
    let line = contents.lines().find(|line| {
        line.to_ascii_lowercase()
            .contains(&marker.to_ascii_lowercase())
            && line.contains("%*")
    })?;
    let start = line
        .to_ascii_lowercase()
        .find(&marker.to_ascii_lowercase())?
        + marker.len();
    let target = line[start..].split_once('"')?.0;
    let relative = target
        .strip_prefix("%dp0%")?
        .trim_start_matches(['\\', '/']);
    let relative = relative.replace('\\', "/");
    Some(parent.join(relative))
}

pub(super) fn platform_key() -> &'static str {
    match std::env::consts::OS {
        "windows" => "windows",
        "macos" => "macos",
        _ => "linux",
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod process_fixture;
