use std::{
    io,
    path::Path,
    process::{Child, Command, Stdio},
};

pub use codeterm_types::server_bootstrap::{InstanceSelector, LaunchOptions};

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

pub fn spawn_detached(executable: &Path) -> io::Result<Child> {
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
