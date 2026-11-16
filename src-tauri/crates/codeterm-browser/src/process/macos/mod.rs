mod events;

use std::os::fd::OwnedFd;
use std::path::Path;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};
use std::time::Duration;

use block2::RcBlock;
use objc2::rc::{autoreleasepool, Retained};
use objc2_app_kit::{NSRunningApplication, NSWorkspace, NSWorkspaceOpenConfiguration};
use objc2_foundation::{NSArray, NSBundle, NSError, NSString, NSURL};
use parking_lot::Mutex;
use serde_json::json;
use tokio::sync::{oneshot, watch};
use tokio::time::Instant;

use super::termination::{TerminationAction, TerminationModel};
use super::{BrowserProcess, ProcessExit, SpawnError, FORCE_TERMINATE_DEADLINE, KILL_DEADLINE};
use crate::cdp::CdpClient;
use crate::launch::MacOsAppLaunch;

struct Application {
    app: Retained<NSRunningApplication>,
    exit_queue: Option<OwnedFd>,
    identity: Option<events::ProcessIdentity>,
    verified: Arc<AtomicU32>,
    kill_sent: bool,
    exited: bool,
}

impl Application {
    fn verified_pid(&self) -> Option<u32> {
        let pid = self.verified.load(Ordering::Acquire);
        (pid > 1).then_some(pid)
    }

    fn sigkill(&mut self) -> std::io::Result<()> {
        self.kill_sent = true;
        self.identity
            .as_ref()
            .ok_or_else(|| std::io::Error::other("browser has no captured process identity"))?
            .kill_verified(self.verified_pid())
    }
}

impl Drop for Application {
    fn drop(&mut self) {
        if !self.exited {
            if self.verified_pid().is_some() && !self.kill_sent {
                if let Err(error) = self.sigkill() {
                    log::warn!("[browser-process] drop termination failed: {error}");
                }
            } else if !self.kill_sent {
                self.app.forceTerminate();
            }
        }
    }
}

pub(super) async fn spawn(
    binary: &Path,
    args: &[String],
    spec: MacOsAppLaunch,
    deadline: Duration,
) -> Result<BrowserProcess, SpawnError> {
    let binary = binary.to_path_buf();
    let args = args.to_vec();
    let (reply, result) = oneshot::channel();
    tokio::spawn(async move {
        let result = spawn_inner(binary.as_path(), &args, spec, deadline).await;
        let _ = reply.send(result);
    });
    result.await.map_err(|error| SpawnError {
        detail: format!("browser app launch task stopped: {error}"),
        process_may_be_running: true,
    })?
}

async fn spawn_inner(
    binary: &Path,
    args: &[String],
    spec: MacOsAppLaunch,
    deadline: Duration,
) -> Result<BrowserProcess, SpawnError> {
    let started = Instant::now();
    let until = started + deadline;
    let profile = spec.profile_dir.clone();
    let port_file = tokio::time::timeout_at(
        until,
        tokio::task::spawn_blocking(move || events::PortFile::watch(&profile)),
    )
    .await
    .map_err(|_| SpawnError::before_launch("browser profile watch setup deadline".into()))?
    .map_err(|error| SpawnError::before_launch(error.to_string()))?
    .map_err(|error| SpawnError::before_launch(format!("browser profile watch: {error}")))?;
    let cancel_port = port_file.cancellation().map_err(|error| {
        SpawnError::before_launch(format!("browser profile cancellation: {error}"))
    })?;
    let (launched, application) = oneshot::channel();
    let binary = binary.to_path_buf();
    let args = args.to_vec();
    tokio::task::spawn_blocking(move || begin_launch(&binary, &args, &spec, launched));
    let mut application = tokio::time::timeout_at(until, application)
        .await
        .map_err(|_| SpawnError {
            detail: "browser app launch deadline".into(),
            process_may_be_running: true,
        })?
        .map_err(|error| SpawnError {
            detail: format!("browser app launch callback stopped: {error}"),
            process_may_be_running: true,
        })?
        .map_err(|detail| SpawnError {
            detail,
            process_may_be_running: true,
        })?;
    let pid = application.app.processIdentifier() as u32;
    let queue = application.exit_queue.take().ok_or_else(|| SpawnError {
        detail: "browser app has no exit observation".into(),
        process_may_be_running: true,
    })?;
    let events = events::ExitEvents::register(queue).map_err(|error| SpawnError {
        detail: format!("browser exit observation: {error}"),
        process_may_be_running: true,
    })?;
    let (exit_tx, exit) = watch::channel(None);
    let (kill, kill_rx) = watch::channel(false);
    let verified = application.verified.clone();
    tokio::spawn(supervise(application, pid, events, kill_rx, exit_tx));
    let mut process = BrowserProcess {
        pid,
        endpoint: String::new(),
        ready_after: Duration::ZERO,
        exit,
        kill,
    };
    let readiness = tokio::task::spawn_blocking(move || port_file.endpoint(until.into_std()));
    let mut exited = process.exit_watch();
    let ready = async {
        tokio::select! {
            result = readiness => result.map_err(|error| error.to_string())?,
            _ = exited.wait_for(Option::is_some) => Err("browser exited before DevTools was ready".into()),
        }
    };
    let result = tokio::time::timeout_at(until, async {
        let endpoint = ready.await?;
        verify_endpoint(&endpoint, pid).await?;
        verified.store(pid, Ordering::Release);
        Ok::<_, String>(endpoint)
    })
    .await
    .unwrap_or_else(|_| Err("browser DevTools readiness deadline".into()));
    drop(cancel_port);
    match result {
        Ok(endpoint) => {
            process.endpoint = endpoint;
            process.ready_after = started.elapsed();
            Ok(process)
        }
        Err(detail) => {
            process.kill();
            let exited = process
                .wait_exit_until(Instant::now() + KILL_DEADLINE)
                .await;
            Err(SpawnError {
                detail,
                process_may_be_running: exited.is_none(),
            })
        }
    }
}

fn begin_launch(
    binary: &Path,
    args: &[String],
    spec: &MacOsAppLaunch,
    reply: oneshot::Sender<Result<Application, String>>,
) {
    autoreleasepool(|_| {
        let reply = Mutex::new(Some(reply));
        let bundle_url = NSURL::fileURLWithPath_isDirectory(
            &NSString::from_str(&spec.bundle.to_string_lossy()),
            true,
        );
        let valid_binary = NSBundle::bundleWithURL(&bundle_url)
            .and_then(|bundle| bundle.executableURL())
            .and_then(|url| url.path())
            .and_then(|path| std::fs::canonicalize(path.to_string()).ok());
        if valid_binary.as_deref() != std::fs::canonicalize(binary).ok().as_deref()
            || valid_binary.is_none()
        {
            if let Some(reply) = reply.lock().take() {
                let _ = reply.send(Err(
                    "configured browser is not the app bundle's main executable".into(),
                ));
            }
            return;
        }
        let configuration = NSWorkspaceOpenConfiguration::new();
        configuration.setAddsToRecentItems(spec.adds_to_recent_items);
        configuration.setCreatesNewApplicationInstance(spec.creates_new_instance);
        configuration.setActivates(spec.activates);
        configuration.setAllowsRunningApplicationSubstitution(false);
        configuration.setPromptsUserIfNeeded(false);
        let args: Vec<_> = args.iter().map(|arg| NSString::from_str(arg)).collect();
        configuration.setArguments(&NSArray::from_retained_slice(&args));
        let callback = RcBlock::new(move |app: *mut NSRunningApplication, error: *mut NSError| {
            let result = unsafe { Retained::retain(app) }
                .ok_or_else(|| {
                    unsafe { error.as_ref() }
                        .map(|error| error.localizedDescription().to_string())
                        .unwrap_or_else(|| "browser app launch returned no application".into())
                })
                .and_then(|app| {
                    let mut application = Application {
                        app,
                        exit_queue: None,
                        identity: None,
                        verified: Arc::new(AtomicU32::new(0)),
                        kill_sent: false,
                        exited: false,
                    };
                    let pid = application.app.processIdentifier();
                    if pid <= 1 {
                        return Err("browser app launch returned an invalid PID".into());
                    }
                    application.exit_queue =
                        Some(events::process_exit(pid as u32).map_err(|error| {
                            format!("browser exited before exit observation: {error}")
                        })?);
                    application.identity = Some(
                        events::ProcessIdentity::capture(pid as u32).map_err(|error| {
                            format!("browser process identity unavailable: {error}")
                        })?,
                    );
                    Ok(application)
                });
            if let Some(reply) = reply.lock().take() {
                let _ = reply.send(result);
            }
        });
        NSWorkspace::sharedWorkspace().openApplicationAtURL_configuration_completionHandler(
            &bundle_url,
            &configuration,
            Some(&callback),
        );
    });
}

struct IdentityConnection(CdpClient);

impl Drop for IdentityConnection {
    fn drop(&mut self) {
        self.0.close("browser launch identity checked");
    }
}

async fn verify_endpoint(endpoint: &str, pid: u32) -> Result<(), String> {
    let client = IdentityConnection(CdpClient::connect(endpoint).await?);
    let result = client.0.send("SystemInfo.getProcessInfo", json!({})).await;
    let result = result.map_err(|error| error.to_string())?;
    if browser_pid_matches(&result, pid) {
        Ok(())
    } else {
        Err("browser DevTools endpoint belongs to a different process".into())
    }
}

fn browser_pid_matches(result: &serde_json::Value, pid: u32) -> bool {
    result
        .get("processInfo")
        .and_then(|value| value.as_array())
        .is_some_and(|rows| {
            rows.iter().any(|row| {
                row.get("type").and_then(|value| value.as_str()) == Some("browser")
                    && row.get("id").and_then(|value| value.as_u64()) == Some(u64::from(pid))
            })
        })
}

async fn supervise(
    mut application: Application,
    pid: u32,
    events: events::ExitEvents,
    mut kill: watch::Receiver<bool>,
    exit: watch::Sender<Option<ProcessExit>>,
) {
    let mut model = TerminationModel::new(pid);
    let kill_requested = async move {
        let _ = kill.wait_for(|requested| *requested).await;
    };
    let result = tokio::select! {
        result = events.exited(pid) => result,
        _ = kill_requested => {
            if model.graceful_deadline() == TerminationAction::ForceTerminate {
                application.app.forceTerminate();
            }
            match tokio::time::timeout(FORCE_TERMINATE_DEADLINE, events.exited(pid)).await {
                Ok(result) => result,
                Err(_) => {
                    if model.force_deadline(application.verified_pid()) == TerminationAction::KillOwnedPid(pid) {
                        if let Err(error) = application.sigkill() {
                            log::warn!("[browser-process] verified SIGKILL {pid} failed: {error}");
                        }
                    }
                    match tokio::time::timeout(KILL_DEADLINE, events.exited(pid)).await {
                        Ok(result) => result,
                        Err(_) => {
                            log::warn!("[browser-process] app {pid} exit unconfirmed; retaining exit observation");
                            events.exited(pid).await
                        }
                    }
                },
            }
        }
    };
    match result {
        Ok(()) => {
            model.observed_exit();
            application.exited = model.may_discard_profile();
            exit.send_replace(Some(ProcessExit { code: None }));
        }
        Err(error) => log::warn!("[browser-process] app {pid} exit observation failed: {error}"),
    }
}

#[cfg(test)]
mod tests;
