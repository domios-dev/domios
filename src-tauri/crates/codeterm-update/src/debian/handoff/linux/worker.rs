use super::super::{
    may_restore_snapshot, relaunch_target, run, Elevation, Plan, Port, Relaunch, Step,
};
use crate::debian::{staging, StageLease};
use codeterm_process_identity::PinnedProcess;
use codeterm_types::app_quit::{QuitApiReply, QuitCompletion};
use std::{
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::net::{UnixListener, UnixStream},
    },
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

pub(super) async fn execute(plan: Plan, leases: Vec<StageLease>) -> Result<(), String> {
    let mut worker = Worker {
        plan,
        leases,
        pins: Vec::new(),
        quit_dispatched: false,
        deadline: None,
        replacement: None,
        replacement_server: None,
        receipt_seen: false,
    };
    run(&mut worker).await
}

struct Worker {
    plan: Plan,
    leases: Vec<StageLease>,
    pins: Vec<PinnedProcess>,
    quit_dispatched: bool,
    deadline: Option<Instant>,
    replacement: Option<Replacement>,
    replacement_server: Option<PinnedProcess>,
    receipt_seen: bool,
}

enum Replacement {
    Service,
    Child(Child),
}

impl Worker {
    fn data(&self) -> &std::path::Path {
        &self.plan.target.ready.identity.data
    }

    fn spawn_relaunch(&mut self, rollback: bool) -> Result<(), String> {
        let mut command = match relaunch_target(rollback, !rollback && unit_exists()) {
            Relaunch::OldSnapshot => {
                let mut command = Command::new(self.plan.directory.join("rollback/domios-app"));
                command.env("DOMIOS_UPDATE_ROLLBACK", &self.plan.directory);
                command
            }
            Relaunch::Service => {
                self.replacement = Some(Replacement::Service);
                let mut command = Command::new("systemctl");
                command.args(["--user", "restart", "domios.service"]);
                return checked(&mut command);
            }
            Relaunch::Installed => Command::new(&self.plan.app),
        };
        command
            .env_remove("DOMIOS_DEB_HANDOFF")
            .env_remove("DOMIOS_DEB_DETACH")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = command.spawn().map_err(|e| e.to_string())?;
        if !rollback {
            self.replacement = Some(Replacement::Child(child));
        }
        Ok(())
    }

    fn relaunch(&mut self) -> Result<(), String> {
        let socket = self.plan.directory.join("started.sock");
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(socket).map_err(|e| e.to_string())?;
        std::fs::write(
            self.data().join("debian-update-starting.json"),
            serde_json::to_vec(&self.plan.directory).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        self.spawn_relaunch(false)?;
        let mut fd = libc::pollfd {
            fd: listener.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        if unsafe { libc::poll(&mut fd, 1, 60_000) } != 1 {
            return Err("replacement pair did not acknowledge startup".into());
        }
        let (stream, _) = listener.accept().map_err(|e| e.to_string())?;
        self.receipt_seen = true;
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        stream
            .take(32769)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 32768 {
            return Err("replacement startup receipt exceeds its bound".into());
        }
        let receipt: super::StartupReceipt =
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if receipt.ready.identity != self.plan.target.ready.identity
            || receipt.ready.process.executable != self.plan.target.ready.process.executable
        {
            return Err("replacement receipt does not belong to the installed pair".into());
        }
        let pin = PinnedProcess::open(receipt.ready.process.pid).map_err(|e| e.to_string())?;
        if pin.provenance().map_err(|e| e.to_string())? != receipt.ready.process {
            return Err("replacement server identity changed".into());
        }
        self.replacement_server = Some(pin);
        if receipt.version != self.plan.version
            || receipt.build != receipt.ready.build
            || receipt.ready.identity != self.plan.target.ready.identity
            || receipt.ready.epoch == self.plan.target.ready.epoch
            || receipt.ready.process == self.plan.target.ready.process
        {
            return Err("replacement startup does not match the verified version and pair".into());
        }
        Ok(())
    }

    fn stop_replacement(&mut self) -> Result<(), String> {
        match &mut self.replacement {
            Some(Replacement::Service) => {
                checked(Command::new("systemctl").args(["--user", "stop", "domios.service"]))?;
                let output = Command::new("systemctl")
                    .args([
                        "--user",
                        "show",
                        "domios.service",
                        "--property=ActiveState",
                        "--value",
                    ])
                    .output()
                    .map_err(|e| e.to_string())?;
                if !output.status.success()
                    || String::from_utf8_lossy(&output.stdout).trim() != "inactive"
                {
                    return Err("replacement service stop is unconfirmed".into());
                }
            }
            Some(Replacement::Child(child)) => {
                if child.try_wait().map_err(|e| e.to_string())?.is_none() {
                    let pin = match PinnedProcess::open(child.id()) {
                        Ok(pin) => Some(pin),
                        Err(error) => {
                            if child.try_wait().map_err(|e| e.to_string())?.is_none() {
                                return Err(error.to_string());
                            }
                            None
                        }
                    };
                    if let Some(pin) = pin {
                        child.kill().map_err(|e| e.to_string())?;
                        pin.wait_exit(Duration::from_secs(30))
                            .map_err(|e| e.to_string())?;
                        child.wait().map_err(|e| e.to_string())?;
                    }
                }
            }
            None => return staging::instance_unused(&self.plan.target.instance_locks),
        }
        if let Some(server) = &self.replacement_server {
            server
                .wait_exit(Duration::from_secs(30))
                .map_err(|e| e.to_string())?;
        } else if self.receipt_seen {
            return Err("replacement server exit cannot be confirmed".into());
        } else {
            staging::instance_unused(&self.plan.target.instance_locks)?;
        }
        Ok(())
    }

    async fn quit(&mut self) -> Result<(), String> {
        let address = self
            .plan
            .target
            .ready
            .loopback_api_address()
            .ok_or("invalid local API")?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| e.to_string())?;
        self.quit_dispatched = true;
        self.deadline = Some(Instant::now() + Duration::from_secs(30));
        let response = client
            .post(format!("http://{address}/api/app/quit"))
            .bearer_auth(&self.plan.target.token)
            .json(&self.plan.snapshot)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?;
        match response
            .json::<QuitApiReply>()
            .await
            .map_err(|e| e.to_string())?
        {
            QuitApiReply::Completed {
                expected,
                shutdown_completion,
            } if expected.matches(&self.plan.snapshot)
                && shutdown_completion == QuitCompletion::default() =>
            {
                Ok(())
            }
            _ => Err("exact application quit was not confirmed".into()),
        }
    }
}

#[async_trait::async_trait]
impl Port for Worker {
    async fn execute(&mut self, step: Step) -> Result<(), String> {
        match step {
            Step::Verify => {
                if staging::digest(&self.plan.deb)? != self.plan.sha256 {
                    return Err("staged SHA256 changed".into());
                }
                Ok(())
            }
            Step::Pin => {
                for expected in [&self.plan.target.ready.process, &self.plan.target.client] {
                    let pin = PinnedProcess::open(expected.pid).map_err(|e| e.to_string())?;
                    if pin.provenance().map_err(|e| e.to_string())? != *expected {
                        return Err("exact process incarnation changed".into());
                    }
                    self.pins.push(pin);
                }
                let mut stream = UnixStream::connect(self.plan.directory.join("ready.sock"))
                    .map_err(|e| e.to_string())?;
                stream
                    .set_read_timeout(Some(Duration::from_secs(30)))
                    .map_err(|e| e.to_string())?;
                stream.write_all(&[1]).map_err(|e| e.to_string())?;
                let mut length = [0; 4];
                stream.read_exact(&mut length).map_err(|e| e.to_string())?;
                let length = u32::from_be_bytes(length) as usize;
                if length == 0 || length > 4096 {
                    return Err("invalid local credential frame".into());
                }
                let mut token = vec![0; length];
                stream.read_exact(&mut token).map_err(|e| e.to_string())?;
                self.plan.target.token = String::from_utf8(token).map_err(|e| e.to_string())?;
                Ok(())
            }
            Step::Quit => self.quit().await,
            Step::Wait => {
                let deadline = self.deadline.ok_or("quit deadline missing")?;
                for pin in &self.pins {
                    pin.wait_exit(deadline.saturating_duration_since(Instant::now()))
                        .map_err(|e| e.to_string())?;
                }
                Ok(())
            }
            Step::Install => {
                if staging::digest(&self.plan.deb)? != self.plan.sha256 {
                    return Err("staged SHA256 changed before dpkg".into());
                }
                let mut command = match self.plan.elevation {
                    Elevation::Pkexec => Command::new("pkexec"),
                    Elevation::SudoNonInteractive => {
                        let mut command = Command::new("sudo");
                        command.arg("-n");
                        command
                    }
                };
                command
                    .args(["dpkg", "-i", "--"])
                    .arg(&self.plan.deb)
                    .env_remove("SUDO_ASKPASS");
                checked(&mut command)
            }
            Step::Relaunch => {
                let _ = std::fs::remove_file(self.data().join("debian-update-failure.txt"));
                self.relaunch()
            }
            Step::Cleanup => {
                if let Some(directory) = self.plan.deb.parent() {
                    if let Err(error) = std::fs::remove_dir_all(directory) {
                        log::debug!("[updater] installed package cleanup failed: {error}");
                    }
                }
                if let Err(error) = std::fs::remove_dir_all(&self.plan.directory) {
                    log::debug!("[updater] installed package cleanup failed: {error}");
                }
                staging::cleanup(&std::env::temp_dir());
                staging::cleanup(&self.data().join("state/update-handoffs"));
                Ok(())
            }
        }
    }

    async fn recover(&mut self, failure: &str) {
        let path = self.data().join("debian-update-failure.txt");
        let _ = std::fs::remove_file(self.data().join("debian-update-starting.json"));
        let _ = std::fs::write(&path, failure);
        if self.quit_dispatched {
            let deadline = Instant::now() + Duration::from_secs(30);
            let old_pair_exited = self.pins.len() == 2
                && self.pins.iter().all(|pin| {
                    pin.wait_exit(deadline.saturating_duration_since(Instant::now()))
                        .is_ok()
                });
            let replacement_stopped = if old_pair_exited {
                self.stop_replacement()
            } else {
                Err("original pair exit is unconfirmed".into())
            };
            if !may_restore_snapshot(old_pair_exited, replacement_stopped.is_ok()) {
                let _ = std::fs::write(
                    &path,
                    format!(
                        "{failure}\nOld version relaunch withheld: {}",
                        replacement_stopped
                            .err()
                            .unwrap_or_else(|| "process exit unconfirmed".into())
                    ),
                );
                return;
            }
            self.leases.clear();
            if let Err(error) = self.spawn_relaunch(true) {
                let _ = std::fs::write(
                    &path,
                    format!("{failure}\nOld version relaunch failed: {error}"),
                );
            }
        }
    }
}

fn unit_exists() -> bool {
    Command::new("systemctl")
        .args([
            "--user",
            "show",
            "domios.service",
            "--property=LoadState",
            "--value",
        ])
        .output()
        .is_ok_and(|output| {
            output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "loaded"
        })
}

fn checked(command: &mut Command) -> Result<(), String> {
    let output = command
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "installer or relaunch exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}
