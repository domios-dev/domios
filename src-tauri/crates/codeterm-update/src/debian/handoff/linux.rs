use super::{inside_systemd_unit, launch_mode, Elevation, Plan, QuitTarget};
use crate::debian::{staging, StagedDebianPackage};
use codeterm_types::app_quit::QuitSnapshot;
use std::{
    io::{Read, Write},
    os::{fd::AsRawFd, unix::net::UnixListener},
    path::Path,
    process::{Command, Stdio},
};

mod worker;

#[derive(serde::Serialize, serde::Deserialize)]
struct StartupReceipt {
    version: String,
    build: String,
    ready: codeterm_types::server_bootstrap::ServerReady,
}

pub fn acknowledge_start(ready: &codeterm_types::server_bootstrap::ServerReady) {
    let pending = ready.identity.data.join("debian-update-starting.json");
    let result = (|| -> Result<(), String> {
        let directory: std::path::PathBuf =
            serde_json::from_slice(&std::fs::read(&pending).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        let path = directory.join("started.sock");
        let mut stream =
            std::os::unix::net::UnixStream::connect(path).map_err(|e| e.to_string())?;
        let receipt = StartupReceipt {
            version: codeterm_build_info::version().into(),
            build: codeterm_build_info::sha().into(),
            ready: ready.clone(),
        };
        stream
            .write_all(&serde_json::to_vec(&receipt).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        Ok(())
    })();
    if result.is_ok() {
        let _ = std::fs::remove_file(pending);
    }
}

pub async fn launch(
    package: StagedDebianPackage,
    target: QuitTarget,
    resources: std::path::PathBuf,
) -> Result<(), String> {
    let address = target
        .ready
        .loopback_api_address()
        .ok_or("update requires a local API")?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let snapshot: QuitSnapshot = client
        .get(format!("http://{address}/api/app/quit/snapshot"))
        .bearer_auth(&target.token)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    if !snapshot_matches(&snapshot, &target) {
        return Err("update quit snapshot differs from attached pair".into());
    }
    tokio::task::spawn_blocking(move || prepare_and_launch(package, target, snapshot, &resources))
        .await
        .map_err(|e| e.to_string())?
}

fn snapshot_matches(snapshot: &QuitSnapshot, target: &QuitTarget) -> bool {
    snapshot.valid()
        && snapshot.ready == target.ready
        && snapshot.client.as_ref().is_some_and(|client| {
            super::exact_pair(
                (&target.ready.process, &target.client),
                (&snapshot.ready.process, &client.process),
            )
        })
}

fn available(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|dir| {
            use std::os::unix::fs::PermissionsExt;
            std::fs::metadata(dir.join(program))
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
    })
}

fn prepare_and_launch(
    package: StagedDebianPackage,
    target: QuitTarget,
    snapshot: QuitSnapshot,
    resources: &Path,
) -> Result<(), String> {
    let graphical = ["DISPLAY", "WAYLAND_DISPLAY"]
        .iter()
        .any(|name| std::env::var_os(name).is_some_and(|v| !v.is_empty()));
    let inside_unit = inside_systemd_unit(
        std::env::var_os("INVOCATION_ID").is_some(),
        &std::fs::read_to_string("/proc/self/cgroup").map_err(|e| e.to_string())?,
    );
    let systemd_available = available("systemd-run")
        && Command::new("systemctl")
            .args(["--user", "show-environment"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
    let (elevation, use_systemd) = launch_mode(
        inside_unit,
        graphical,
        available("pkexec"),
        available("sudo"),
        systemd_available,
    )?;
    if elevation == Elevation::SudoNonInteractive {
        let status = Command::new("sudo")
            .args(["-n", "true"])
            .env_remove("SUDO_ASKPASS")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|e| e.to_string())?;
        if !status.success() {
            return Err(
                "sudo -n authorization is unavailable; the session is still running".into(),
            );
        }
    }
    let root = target.ready.identity.data.join("state/update-handoffs");
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let execution = tempfile::Builder::new()
        .prefix("domios-verified-update-")
        .tempdir_in(&root)
        .map_err(|e| e.to_string())?;
    let directory = execution.path().to_path_buf();
    let app = std::env::current_exe().map_err(|e| e.to_string())?;
    let rollback = directory.join("rollback");
    std::fs::create_dir_all(&rollback).map_err(|e| e.to_string())?;
    std::fs::copy(&app, rollback.join("domios-app")).map_err(|e| e.to_string())?;
    copy_tree(resources, &rollback.join("resources"))?;
    let helper = directory.join("handoff-app");
    std::fs::copy(&app, &helper).map_err(|e| e.to_string())?;
    let plan = Plan {
        directory: directory.clone(),
        deb: package.path.clone(),
        sha256: package.sha256.clone(),
        version: package.version.clone(),
        app,
        elevation,
        target,
        snapshot,
    };
    let plan_path = directory.join("handoff.json");
    std::fs::write(
        &plan_path,
        serde_json::to_vec(&plan).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let socket = directory.join("ready.sock");
    let _ = std::fs::remove_file(&socket);
    let listener = UnixListener::bind(&socket).map_err(|e| e.to_string())?;
    let mut command = if use_systemd {
        let mut cmd = Command::new("systemd-run");
        cmd.args(["--user", "--collect", "--quiet"])
            .arg(format!("--unit=domios-update-{}", uuid::Uuid::new_v4()))
            .arg(format!(
                "--setenv=DOMIOS_DEB_HANDOFF={}",
                plan_path.display()
            ));
        for name in [
            "DISPLAY",
            "WAYLAND_DISPLAY",
            "XAUTHORITY",
            "DBUS_SESSION_BUS_ADDRESS",
            "XDG_RUNTIME_DIR",
        ] {
            if let Some(value) = std::env::var_os(name) {
                let mut arg = std::ffi::OsString::from(format!("--setenv={name}="));
                arg.push(value);
                cmd.arg(arg);
            }
        }
        cmd.arg(&helper);
        cmd
    } else {
        let mut cmd = Command::new(&helper);
        cmd.env("DOMIOS_DEB_DETACH", "1");
        cmd
    };
    command
        .env("DOMIOS_DEB_HANDOFF", &plan_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let _ = execution.keep();
    drop(package.lease);
    if !command.status().map_err(|e| e.to_string())?.success() {
        return Err("detached Debian helper launch refused".into());
    }
    let mut fd = libc::pollfd {
        fd: listener.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    if unsafe { libc::poll(&mut fd, 1, 30_000) } != 1 {
        return Err("detached Debian helper did not acknowledge readiness".into());
    }
    let (mut stream, _) = listener.accept().map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(30)))
        .map_err(|e| e.to_string())?;
    let mut ack = [0];
    stream.read_exact(&mut ack).map_err(|e| e.to_string())?;
    if ack != [1] {
        return Err("detached Debian helper preflight failed".into());
    }
    let token = plan.target.token.as_bytes();
    if token.is_empty() || token.len() > 4096 {
        return Err("invalid local update credential length".into());
    }
    stream
        .write_all(&(token.len() as u32).to_be_bytes())
        .map_err(|e| e.to_string())?;
    stream.write_all(token).map_err(|e| e.to_string())?;
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    std::fs::create_dir_all(destination).map_err(|e| e.to_string())?;
    for entry in std::fs::read_dir(source).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let target = destination.join(entry.file_name());
        let kind = entry.file_type().map_err(|e| e.to_string())?;
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            std::fs::copy(entry.path(), target).map_err(|e| e.to_string())?;
        } else {
            return Err("update rollback snapshot refuses symlink resources".into());
        }
    }
    Ok(())
}

pub fn entry() -> Option<i32> {
    let path = std::env::var_os("DOMIOS_DEB_HANDOFF")?;
    if std::env::var_os("DOMIOS_DEB_DETACH").is_some() {
        unsafe {
            match libc::fork() {
                -1 => return Some(1),
                0 => {}
                _ => return Some(0),
            }
            if libc::setsid() < 0 {
                libc::_exit(1);
            }
            match libc::fork() {
                -1 => libc::_exit(1),
                0 => {}
                _ => libc::_exit(0),
            }
        }
    }
    let outcome = (|| {
        let plan: Plan = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let leases = leases(&plan)?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| e.to_string())?;
        runtime.block_on(worker::execute(plan, leases))
    })();
    Some(if outcome.is_ok() { 0 } else { 1 })
}

fn leases(plan: &Plan) -> Result<Vec<staging::StageLease>, String> {
    Ok(vec![
        staging::StageLease::acquire(&plan.directory)?,
        staging::StageLease::acquire(plan.deb.parent().ok_or("staged directory missing")?)?,
    ])
}

pub fn rollback_leases(directory: &Path) -> Result<Vec<staging::StageLease>, String> {
    let plan: Plan = serde_json::from_slice(
        &std::fs::read(directory.join("handoff.json")).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    if plan.directory != directory {
        return Err("rollback directory differs from its plan".into());
    }
    let mut leases = vec![staging::StageLease::acquire(&plan.directory)?];
    if let Some(staged) = plan.deb.parent() {
        if let Ok(lease) = staging::StageLease::acquire(staged) {
            leases.push(lease);
        }
    }
    Ok(leases)
}
