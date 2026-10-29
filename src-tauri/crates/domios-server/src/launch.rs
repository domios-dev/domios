use std::{
    io::{self, BufRead, Read, Write},
    net::{Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

use codeterm_types::server_bootstrap::{AttachRequest, BootstrapOperation, ServerExpectation};
pub use codeterm_types::server_bootstrap::{InstanceSelector, LaunchOptions};

mod detached;
pub mod pid_file;
#[cfg(windows)]
use detached::retry_without_breakaway;
pub use detached::{ensure_detached, release_launch_stderr, JobMembership};
use detached::{with_stderr_detail, StderrTail};

const STDERR_CLOSE_DEADLINE: std::time::Duration = std::time::Duration::from_secs(2);

pub const HUMAN_START_USAGE: &str = "domios-server start --production [--resources PATH] [--pid-file ABS_PATH] [--json]\nDebug artifact: domios-server start --development --port PORT [--instance NAME] [--resources PATH] [--json]\nIsolated verification: domios-server start --verify-isolated --data-dir ABS_DIR --config-dir ABS_DIR --port PORT";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HumanStartMode {
    Detached,
    IsolatedVerification,
}

pub struct HumanStart {
    pub options: LaunchOptions,
    pub json: bool,
    pub mode: HumanStartMode,
    pub pid_file: Option<PathBuf>,
}

pub fn human_start_options(
    args: &[String],
    executable: &Path,
    development_artifact: bool,
) -> io::Result<HumanStart> {
    if args.first().is_some_and(|arg| arg == "--verify-isolated") {
        return isolated_verification_options(args, executable, development_artifact);
    }
    let mut selector = None;
    let mut instance = None;
    let mut port = None;
    let mut resources = None;
    let mut json = false;
    let mut pid_file = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--production" | "--development" if selector.is_none() => {
                selector = Some(arg == "--development");
            }
            "--resources" if resources.is_none() => {
                resources = Some(PathBuf::from(human_value(&mut args)?));
            }
            "--instance" if instance.is_none() => {
                instance = Some(human_value(&mut args)?.to_owned());
            }
            "--port" if port.is_none() => {
                port =
                    Some(human_value(&mut args)?.parse::<u16>().map_err(|_| {
                        io::Error::other("Headless development port must be a u16")
                    })?);
            }
            "--json" if !json => json = true,
            "--pid-file" if pid_file.is_none() => {
                let path = PathBuf::from(human_value(&mut args)?);
                if !path.is_absolute() {
                    return Err(io::Error::other("PID file path must be absolute"));
                }
                pid_file = Some(path);
            }
            _ => return Err(io::Error::other("Unknown or repeated headless option")),
        }
    }
    let instance = match selector {
        Some(true) => InstanceSelector::Development {
            instance,
            port: port.ok_or_else(|| io::Error::other("Development requires --port"))?,
        },
        Some(false) if instance.is_none() && port.is_none() => InstanceSelector::Production,
        _ => {
            return Err(io::Error::other(
                "Select exactly one explicit headless instance",
            ))
        }
    };
    let resources = match resources {
        Some(path) => path,
        None => common_resources(executable)?,
    };
    let options = LaunchOptions {
        instance,
        resources,
        probe_sink: None,
        probe_identity_alias: None,
    };
    options.validate(development_artifact)?;
    Ok(HumanStart {
        options,
        json,
        mode: HumanStartMode::Detached,
        pid_file,
    })
}

fn isolated_verification_options(
    args: &[String],
    executable: &Path,
    development_artifact: bool,
) -> io::Result<HumanStart> {
    let mut verify = false;
    let mut data_dir = None;
    let mut config_dir = None;
    let mut port = None;
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--verify-isolated" if !verify => verify = true,
            "--data-dir" if data_dir.is_none() => {
                data_dir = Some(PathBuf::from(human_value(&mut args)?));
            }
            "--config-dir" if config_dir.is_none() => {
                config_dir = Some(PathBuf::from(human_value(&mut args)?));
            }
            "--port" if port.is_none() => {
                port =
                    Some(human_value(&mut args)?.parse::<u16>().map_err(|_| {
                        io::Error::other("Isolated verification port must be a u16")
                    })?);
            }
            _ => {
                return Err(io::Error::other(
                    "Unknown or repeated isolated verification option",
                ))
            }
        }
    }
    if !verify {
        return Err(io::Error::other(
            "Isolated verification mode must be explicit",
        ));
    }
    let options = LaunchOptions {
        instance: InstanceSelector::IsolatedVerification {
            data_dir: data_dir
                .ok_or_else(|| io::Error::other("Verification requires --data-dir"))?,
            config_dir: config_dir
                .ok_or_else(|| io::Error::other("Verification requires --config-dir"))?,
            bind: SocketAddr::new(
                Ipv4Addr::LOCALHOST.into(),
                port.ok_or_else(|| io::Error::other("Verification requires --port"))?,
            ),
        },
        resources: common_resources(executable)?,
        probe_sink: None,
        probe_identity_alias: None,
    };
    options.validate(development_artifact)?;
    Ok(HumanStart {
        options,
        json: false,
        mode: HumanStartMode::IsolatedVerification,
        pid_file: None,
    })
}

fn common_resources(executable: &Path) -> io::Result<PathBuf> {
    executable
        .parent()
        .and_then(Path::parent)
        .map(Path::to_owned)
        .ok_or_else(|| io::Error::other("Common resource root unavailable"))
}

fn human_value<'a>(args: &mut std::slice::Iter<'a, String>) -> io::Result<&'a str> {
    args.next()
        .map(String::as_str)
        .filter(|value| !value.is_empty() && !value.starts_with("--"))
        .ok_or_else(|| io::Error::other("Headless option requires a value"))
}

pub fn write_human_ready(
    output: &mut impl Write,
    ready: &crate::bootstrap::protocol::ServerReady,
    json: bool,
) -> io::Result<()> {
    if json {
        serde_json::to_writer(&mut *output, ready).map_err(io::Error::other)?;
        output.write_all(b"\n")?;
    } else {
        writeln!(output, "Domios server ready: http://{}/", ready.api_address)?;
        writeln!(output, "PID: {}  Epoch: {}", ready.process.pid, ready.epoch)?;
        writeln!(output, "Executable: {}", ready.process.executable.display())?;
        writeln!(output, "Build: {}  API: {}", ready.build, ready.api_version)?;
        writeln!(output, "Data: {}", ready.identity.data.display())?;
        writeln!(output, "Config: {}", ready.identity.config.display())?;
    }
    output.flush()
}

pub fn write_isolated_ready(
    output: &mut impl Write,
    ready: &crate::bootstrap::protocol::ServerReady,
) -> io::Result<()> {
    if !ready.api_address.ip().is_loopback() {
        return Err(io::Error::other(
            "Isolated verification server did not bind loopback",
        ));
    }
    serde_json::to_writer(
        &mut *output,
        &serde_json::json!({
            "type": "READY",
            "build": ready.build,
            "port": ready.api_address.port(),
        }),
    )
    .map_err(io::Error::other)?;
    output.write_all(b"\n")?;
    output.flush()
}

pub fn launch_service(
    executable: &Path,
    options: &LaunchOptions,
) -> io::Result<crate::bootstrap::protocol::AttachReply> {
    let (child, reply) = launch_child(executable, options)?;
    drop(child);
    Ok(reply)
}

fn launch_child(
    executable: &Path,
    options: &LaunchOptions,
) -> io::Result<(Child, crate::bootstrap::protocol::AttachReply)> {
    use crate::bootstrap::{process::PinnedProcess, protocol::FAILURE_DEADLINE};
    options.validate(cfg!(debug_assertions))?;
    let executable = executable.canonicalize()?;
    let mut child = if matches!(
        &options.instance,
        InstanceSelector::IsolatedVerification { .. }
    ) {
        spawn_supervised(&executable)?
    } else {
        spawn_detached(&executable)?
    };
    let stderr = StderrTail::follow(child.stderr.take());
    let result = (|| {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("Child bootstrap pipe unavailable"))?;
        serde_json::to_writer(&mut stdin, options).map_err(io::Error::other)?;
        stdin.write_all(b"\n")?;
        drop(stdin);
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("Child readiness pipe unavailable"))?;
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let result = io::BufReader::new(stdout)
                .take(32769)
                .read_line(&mut line)
                .map(|_| line);
            let _ = sender.send(result);
        });
        let line = match receiver.recv_timeout(FAILURE_DEADLINE) {
            Ok(Ok(line)) if !line.is_empty() && line.len() <= 32768 => line,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Server launch failed before readiness",
                ));
            }
        };
        let reply: crate::bootstrap::protocol::AttachReply =
            serde_json::from_str(&line).map_err(io::Error::other)?;
        let pin = PinnedProcess::open(child.id())?;
        let observed = pin.provenance()?;
        if reply.ready.process != observed || observed.executable != executable {
            return Err(io::Error::other("Launcher/server provenance mismatch"));
        }
        Ok(reply)
    })();
    match result {
        Ok(reply) => Ok((child, reply)),
        Err(error) => {
            if child.try_wait()?.is_none() {
                child.kill()?;
                child.wait()?;
            }
            Err(with_stderr_detail(
                error,
                stderr.last_line(STDERR_CLOSE_DEADLINE),
            ))
        }
    }
}

/// The server reports the resolved config file, so the expectation goes through the same resolver.
fn isolated_identity(
    data_dir: &Path,
    config_dir: &Path,
) -> io::Result<codeterm_types::server_bootstrap::ServerIdentity> {
    let data_dir = data_dir.canonicalize()?;
    let config = config_dir.canonicalize()?.join("config.toml");
    let resolved = codeterm_service_core::ResolvedInstancePaths::resolve(&data_dir, &config)?;
    Ok(codeterm_types::server_bootstrap::ServerIdentity {
        data: resolved.data().into(),
        config: resolved.config().into(),
    })
}

pub fn run_isolated_verification(executable: &Path, options: &LaunchOptions) -> io::Result<()> {
    let InstanceSelector::IsolatedVerification {
        data_dir,
        config_dir,
        bind,
    } = &options.instance
    else {
        return Err(io::Error::other(
            "Isolated verification requires its explicit launch mode",
        ));
    };
    let expected = isolated_identity(data_dir, config_dir)?;
    let (mut child, reply) = launch_child(executable, options)?;
    let ready_matches = reply.ready.identity == expected && reply.ready.api_address == *bind;
    if !ready_matches {
        stop_child_after_failure(&mut child, &reply.ready, executable);
        return Err(io::Error::other(
            "Isolated verification readiness does not match its requested identity",
        ));
    }
    if let Err(error) = write_isolated_ready(&mut io::stdout().lock(), &reply.ready) {
        stop_child_after_failure(&mut child, &reply.ready, executable);
        return Err(error);
    }

    let wait_result = wait_for_isolated_verification_stop();
    let shutdown_result = request_server_shutdown(&reply.ready, executable);
    if let Err(error) = shutdown_result {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error);
    }
    let status = child.wait()?;
    wait_result?;
    if !status.success() {
        return Err(io::Error::other(
            "Isolated verification server exited unsuccessfully",
        ));
    }
    Ok(())
}

fn stop_child_after_failure(
    child: &mut Child,
    ready: &crate::bootstrap::protocol::ServerReady,
    executable: &Path,
) {
    if request_server_shutdown(ready, executable).is_err() {
        let _ = child.kill();
    }
    let _ = child.wait();
}

fn request_server_shutdown(
    ready: &crate::bootstrap::protocol::ServerReady,
    executable: &Path,
) -> io::Result<()> {
    use crate::bootstrap::{
        channel,
        protocol::{BOOTSTRAP_PROTOCOL, FAILURE_DEADLINE},
    };
    let request = AttachRequest {
        protocol: BOOTSTRAP_PROTOCOL,
        api_version: ready.api_version,
        build: ready.build.clone(),
        identity: ready.identity.clone(),
        challenge: uuid::Uuid::new_v4().to_string(),
        operation: BootstrapOperation::Shutdown,
        expected: Some(ServerExpectation {
            process: ready.process.clone(),
            epoch: ready.epoch.clone(),
        }),
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let (_, process) = runtime.block_on(channel::attach(&request, executable))?;
    process.wait_exit(FAILURE_DEADLINE)
}

fn wait_for_isolated_verification_stop() -> io::Result<()> {
    use tokio::io::AsyncReadExt;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let mut stdin = tokio::io::stdin();
        let mut bytes = [0u8; 256];
        let interrupt = tokio::signal::ctrl_c();
        tokio::pin!(interrupt);
        #[cfg(unix)]
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        loop {
            #[cfg(unix)]
            tokio::select! {
                read = stdin.read(&mut bytes) => {
                    if read? == 0 {
                        return Ok(());
                    }
                }
                result = &mut interrupt => return result,
                signal = terminate.recv() => {
                    if signal.is_some() {
                        return Ok(());
                    }
                }
            }
            #[cfg(windows)]
            tokio::select! {
                read = stdin.read(&mut bytes) => {
                    if read? == 0 {
                        return Ok(());
                    }
                }
                result = &mut interrupt => return result,
            }
        }
    })
}

pub(crate) fn verify_identity(
    options: &LaunchOptions,
    normal: codeterm_service_core::ResolvedInstancePaths,
) -> io::Result<codeterm_service_core::ResolvedInstancePaths> {
    if let Some(sink) = &options.probe_sink {
        let isolated = matches!(&options.instance, InstanceSelector::Development { instance: Some(instance), .. } if instance.starts_with("split-oracle-"));
        let resolved = codeterm_service_core::ResolvedInstancePaths::resolve(normal.data(), sink)?;
        if !isolated || resolved.config() != normal.data().join("process-init.jsonl") {
            return Err(io::Error::other(
                "Probe sink must belong to its disposable isolated instance",
            ));
        }
    }
    if let Some(alias) = &options.probe_identity_alias {
        options.validate(cfg!(debug_assertions))?;
        let candidate =
            codeterm_service_core::ResolvedInstancePaths::resolve(&alias.data, &alias.config)?;
        if candidate != normal {
            return Err(io::Error::other(
                "Probe alias differs from configured data/config identity",
            ));
        }
    }
    Ok(normal)
}

pub fn install_selectors(options: &LaunchOptions) -> io::Result<()> {
    options.validate(cfg!(debug_assertions))?;
    for key in [
        "CODETERM_API_URL",
        "CODETERM_API_PORT",
        "CODETERM_API_TOKEN",
        "CODETERM_SOCKET_PATH",
        "CODETERM_INSTANCE",
        "CODETERM_DATA_DIR",
        "CODETERM_CONFIG_DIR",
        "CODETERM_VERIFY_ISOLATED",
        "CODETERM_DEV",
        "CODETERM_DEV_INSTANCE",
        "CODETERM_DEV_API_PORT",
        "CODETERM_WEB_DIST",
        "CODETERM_CONPTY_DIR",
        "CODETERM_MACHINE_OWNER",
        "CODETERM_CLI_PATH",
    ] {
        std::env::remove_var(key);
    }
    match &options.instance {
        InstanceSelector::Production => {}
        InstanceSelector::Development { instance, port } => {
            std::env::set_var("CODETERM_DEV", "1");
            std::env::set_var("CODETERM_API_PORT", port.to_string());
            std::env::set_var("CODETERM_MACHINE_OWNER", "false");
            if let Some(instance) = instance {
                std::env::set_var("CODETERM_INSTANCE", instance);
            }
        }
        InstanceSelector::IsolatedVerification {
            data_dir,
            config_dir,
            bind,
        } => {
            let data_dir = data_dir.canonicalize()?;
            let config_dir = config_dir.canonicalize()?;
            std::env::set_var("CODETERM_DATA_DIR", data_dir);
            std::env::set_var("CODETERM_CONFIG_DIR", config_dir);
            std::env::set_var("CODETERM_API_PORT", bind.port().to_string());
            std::env::set_var("CODETERM_VERIFY_ISOLATED", "1");
            std::env::set_var("CODETERM_MACHINE_OWNER", "false");
        }
    }
    Ok(())
}

#[cfg(windows)]
fn exclude_bootstrap_handle_from_children(
    handle: windows_sys::Win32::Foundation::HANDLE,
) -> io::Result<()> {
    use windows_sys::Win32::Foundation::{SetHandleInformation, HANDLE_FLAG_INHERIT};
    if unsafe { SetHandleInformation(handle, HANDLE_FLAG_INHERIT, 0) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(windows)]
fn exclude_bootstrap_stdio_from_children() -> io::Result<()> {
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE},
    };
    for kind in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        let handle = unsafe { GetStdHandle(kind) };
        if !handle.is_null() && handle != INVALID_HANDLE_VALUE {
            exclude_bootstrap_handle_from_children(handle)?;
        }
    }
    Ok(())
}

pub fn spawn_detached(executable: &Path) -> io::Result<Child> {
    #[cfg(windows)]
    exclude_bootstrap_stdio_from_children()?;
    let mut command = Command::new(executable.canonicalize()?);
    command
        .arg("serve")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(io::Error::last_os_error());
                }
                libc::signal(libc::SIGHUP, libc::SIG_IGN);
                libc::signal(libc::SIGPIPE, libc::SIG_IGN);
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::{
            CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW,
        };
        command.creation_flags(
            CREATE_BREAKAWAY_FROM_JOB | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW,
        );
        command.spawn().or_else(|error| {
            if !retry_without_breakaway(&error) {
                return Err(error);
            }
            command.creation_flags(CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW);
            command.spawn()
        })
    }
    #[cfg(not(windows))]
    command.spawn()
}

fn spawn_supervised(executable: &Path) -> io::Result<Child> {
    let mut command = Command::new(executable.canonicalize()?);
    command
        .arg("serve")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.spawn()
}

#[cfg(test)]
mod tests;
