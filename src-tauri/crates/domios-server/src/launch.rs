use std::{
    io::{self, BufRead, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

pub use codeterm_types::server_bootstrap::{InstanceSelector, LaunchOptions};

pub const HUMAN_START_USAGE: &str = "domios-server start --production [--resources PATH] [--json]\nDebug artifact: domios-server start --development --port PORT [--instance NAME] [--resources PATH] [--json]";

pub struct HumanStart {
    pub options: LaunchOptions,
    pub json: bool,
}

pub fn human_start_options(
    args: &[String],
    executable: &Path,
    development_artifact: bool,
) -> io::Result<HumanStart> {
    let mut selector = None;
    let mut instance = None;
    let mut port = None;
    let mut resources = None;
    let mut json = false;
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
        None => executable
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| io::Error::other("Common resource root unavailable"))?
            .to_owned(),
    };
    let options = LaunchOptions {
        instance,
        resources,
        probe_sink: None,
        probe_identity_alias: None,
    };
    options.validate(development_artifact)?;
    Ok(HumanStart { options, json })
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

pub fn launch_service(
    executable: &Path,
    options: &LaunchOptions,
) -> io::Result<crate::bootstrap::protocol::AttachReply> {
    use crate::bootstrap::{process::PinnedProcess, protocol::FAILURE_DEADLINE};
    options.validate(cfg!(debug_assertions))?;
    let executable = executable.canonicalize()?;
    let mut child = spawn_detached(&executable)?;
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
    if result.is_err() && child.try_wait()?.is_none() {
        child.kill()?;
        child.wait()?;
    }
    result
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
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use windows_sys::Win32::System::Threading::{
            CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW,
        };
        command.creation_flags(
            CREATE_BREAKAWAY_FROM_JOB | CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW,
        );
    }
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
    command.spawn()
}

pub fn ensure_detached() -> io::Result<()> {
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::{
            JobObjects::IsProcessInJob, Threading::GetCurrentProcess,
        };
        let mut in_job = 0;
        if unsafe { IsProcessInJob(GetCurrentProcess(), std::ptr::null_mut(), &mut in_job) } == 0 {
            return Err(io::Error::last_os_error());
        }
        if in_job != 0 {
            return Err(io::Error::other("Server still belongs to a launching job"));
        }
    }
    #[cfg(unix)]
    if unsafe { libc::getsid(0) != libc::getpid() } {
        return Err(io::Error::other("Server does not own its detached session"));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
