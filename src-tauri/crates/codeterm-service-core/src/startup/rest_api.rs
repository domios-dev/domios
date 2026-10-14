use std::collections::VecDeque;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::Serialize;
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{broadcast, mpsc, oneshot, watch};

pub(crate) mod listeners;
mod restart;
mod streams;
pub(crate) use restart::RestartAcceptance;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RestApiPhase {
    Starting,
    Listening,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RestApiDiagnostic {
    ListenerExited,
    BindOwnershipFailed,
    WorkersBlocked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct RestApiStatus {
    pub(crate) generation: u64,
    pub(crate) phase: RestApiPhase,
    pub(crate) diagnostic: Option<RestApiDiagnostic>,
    pub(crate) message: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RestApiRecovery {
    pub(crate) transition_id: u64,
    pub(crate) status: RestApiStatus,
}

/// A configured bind the live socket does not serve. Exists because the bind is
/// snapshotted at process start, so a config write cannot move it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct PendingBind {
    pub(crate) live: String,
    pub(crate) configured: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub(crate) struct RestApiRuntimePolicy {
    pub(crate) enabled: bool,
    pub(crate) cors: bool,
    pub(crate) cors_allow_origins: Vec<String>,
}

#[derive(Clone)]
struct RestApiLifecycle {
    status: RestApiStatus,
    listener_owned: bool,
    recovery_transition: u64,
    bound_addr: Option<SocketAddr>,
    runtime_policy: RestApiRuntimePolicy,
    pending_restart: Option<u64>,
}

#[derive(Clone)]
pub(crate) struct RestApiControl {
    pub(crate) listeners: listeners::ListenerPublication,
    transition: Arc<Mutex<()>>,
    sender: Arc<Mutex<Option<mpsc::UnboundedSender<RestApiCommand>>>>,
    lifecycle: Arc<Mutex<RestApiLifecycle>>,
    status_events: watch::Sender<RestApiStatus>,
    lifecycle_events: broadcast::Sender<RestApiStatus>,
    recovery_events: mpsc::UnboundedSender<RestApiRecovery>,
    recovery_receiver: Arc<Mutex<Option<mpsc::UnboundedReceiver<RestApiRecovery>>>>,
    #[cfg(test)]
    restart_barrier: Arc<Mutex<Option<Arc<tokio::sync::Barrier>>>>,
    #[cfg(test)]
    port_release_barrier: Arc<Mutex<Option<Arc<tokio::sync::Barrier>>>>,
    #[cfg(test)]
    server_exit: Arc<Mutex<Option<oneshot::Receiver<()>>>>,
    #[cfg(test)]
    shutdown_error: Arc<AtomicBool>,
    #[cfg(test)]
    bind_error: Arc<AtomicBool>,
    #[cfg(test)]
    initial_listener: Arc<Mutex<Option<std::net::TcpListener>>>,
}

#[derive(Debug)]
enum RestApiCommand {
    PortReleased,
    Restart {
        completed: oneshot::Sender<RestApiStatus>,
    },
    PrepareRestart {
        accepted: oneshot::Sender<Result<u64, String>>,
        response_complete: oneshot::Receiver<()>,
    },
    ReservedRestart {
        generation: u64,
        completed: oneshot::Sender<RestApiStatus>,
    },
    #[cfg(test)]
    Shutdown {
        completed: oneshot::Sender<()>,
    },
}

enum GenerationEvent {
    Replace,
    ServerExited,
    Restart(oneshot::Sender<RestApiStatus>),
    #[cfg(test)]
    Shutdown(oneshot::Sender<()>),
}

impl RestApiControl {
    pub(crate) fn new() -> Self {
        let status = RestApiStatus {
            generation: 0,
            phase: RestApiPhase::Starting,
            diagnostic: None,
            message: None,
        };
        let (status_events, _) = watch::channel(status.clone());
        let (lifecycle_events, _) = broadcast::channel(32);
        let (recovery_events, recovery_receiver) = mpsc::unbounded_channel();
        Self {
            listeners: listeners::ListenerPublication::default(),
            transition: Arc::new(Mutex::new(())),
            sender: Arc::new(Mutex::new(None)),
            lifecycle: Arc::new(Mutex::new(RestApiLifecycle {
                status,
                listener_owned: false,
                recovery_transition: 0,
                bound_addr: None,
                runtime_policy: RestApiRuntimePolicy::default(),
                pending_restart: None,
            })),
            status_events,
            lifecycle_events,
            recovery_events,
            recovery_receiver: Arc::new(Mutex::new(Some(recovery_receiver))),
            #[cfg(test)]
            restart_barrier: Arc::new(Mutex::new(None)),
            #[cfg(test)]
            port_release_barrier: Arc::new(Mutex::new(None)),
            #[cfg(test)]
            server_exit: Arc::new(Mutex::new(None)),
            #[cfg(test)]
            shutdown_error: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            bind_error: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            initial_listener: Arc::new(Mutex::new(None)),
        }
    }

    pub(crate) fn port_released(&self) -> Result<(), String> {
        self.signal(RestApiCommand::PortReleased)
    }

    fn signal(&self, command: RestApiCommand) -> Result<(), String> {
        self.sender
            .lock()
            .as_ref()
            .ok_or_else(|| "REST API listener is not configured".to_string())?
            .send(command)
            .map_err(|_| "REST API listener controller stopped; restart Domios".to_string())
    }

    fn install(&self, sender: mpsc::UnboundedSender<RestApiCommand>) {
        *self.sender.lock() = Some(sender);
    }

    fn publish_locked(
        &self,
        lifecycle: &mut RestApiLifecycle,
        phase: RestApiPhase,
        diagnostic: Option<RestApiDiagnostic>,
        message: Option<String>,
    ) -> RestApiStatus {
        let status = RestApiStatus {
            generation: lifecycle.status.generation,
            phase,
            diagnostic,
            message,
        };
        lifecycle.status = status.clone();
        self.status_events.send_replace(status.clone());
        let _ = self.lifecycle_events.send(status.clone());
        status
    }

    fn begin_generation(&self) -> u64 {
        let _transition = self.transition.lock();
        self.listeners.publish(Vec::new());
        let mut lifecycle = self.lifecycle.lock();
        lifecycle.status.generation += 1;
        if lifecycle
            .pending_restart
            .is_some_and(|reserved| reserved <= lifecycle.status.generation)
        {
            lifecycle.pending_restart = None;
        }
        lifecycle.listener_owned = false;
        lifecycle.bound_addr = None;
        let generation = lifecycle.status.generation;
        self.publish_locked(&mut lifecycle, RestApiPhase::Starting, None, None);
        generation
    }

    fn listener_bound(&self, generation: u64, addr: SocketAddr) {
        let mut lifecycle = self.lifecycle.lock();
        if lifecycle.status.generation == generation {
            lifecycle.listener_owned = true;
            lifecycle.bound_addr = Some(addr);
        }
    }

    /// The address the process actually holds a socket on, or None while no
    /// generation owns a listener.
    pub(crate) fn bound_addr(&self) -> Option<SocketAddr> {
        let lifecycle = self.lifecycle.lock();
        lifecycle.bound_addr.filter(|_| lifecycle.listener_owned)
    }

    pub(crate) fn set_runtime_policy(&self, policy: RestApiRuntimePolicy) {
        self.lifecycle.lock().runtime_policy = policy;
    }

    pub(crate) fn runtime_policy(&self) -> RestApiRuntimePolicy {
        self.lifecycle.lock().runtime_policy.clone()
    }

    /// None only when the live socket is provably serving `configured`; anything
    /// unproven is reported pending rather than assumed applied.
    pub(crate) fn pending_bind(&self, configured: &str) -> Option<PendingBind> {
        let live = self.bound_addr()?;
        if configured
            .parse::<SocketAddr>()
            .is_ok_and(|configured| configured == live)
        {
            return None;
        }
        Some(PendingBind {
            live: live.to_string(),
            configured: configured.to_string(),
        })
    }

    fn mark_listening(&self, generation: u64, ready: Vec<SocketAddr>) -> Option<RestApiStatus> {
        let _transition = self.transition.lock();
        let mut lifecycle = self.lifecycle.lock();
        if lifecycle.status.generation != generation || !lifecycle.listener_owned {
            return None;
        }
        let status = self.publish_locked(&mut lifecycle, RestApiPhase::Listening, None, None);
        if generation > 1 {
            self.publish_recovery_locked(&mut lifecycle, status.clone());
        }
        drop(lifecycle);
        self.listeners.publish(ready);
        Some(status)
    }

    fn publish_recovery_locked(&self, lifecycle: &mut RestApiLifecycle, status: RestApiStatus) {
        lifecycle.recovery_transition += 1;
        let _ = self.recovery_events.send(RestApiRecovery {
            transition_id: lifecycle.recovery_transition,
            status,
        });
    }

    fn fail_generation(
        &self,
        generation: u64,
        diagnostic: RestApiDiagnostic,
        message: String,
        listener_owned: bool,
    ) -> Option<RestApiStatus> {
        let _transition = self.transition.lock();
        let mut lifecycle = self.lifecycle.lock();
        if lifecycle.status.generation != generation {
            return None;
        }
        lifecycle.listener_owned = listener_owned;
        let status = self.publish_locked(
            &mut lifecycle,
            RestApiPhase::Failed,
            Some(diagnostic),
            Some(message),
        );
        drop(lifecycle);
        self.listeners.publish(Vec::new());
        Some(status)
    }

    pub(crate) fn record_unanswered_probe(&self) -> RestApiDiagnostic {
        let _transition = self.transition.lock();
        let mut lifecycle = self.lifecycle.lock();
        let diagnostic = if lifecycle.listener_owned {
            if lifecycle.status.phase != RestApiPhase::Failed
                || lifecycle.status.diagnostic != Some(RestApiDiagnostic::WorkersBlocked)
            {
                self.publish_locked(
                    &mut lifecycle,
                    RestApiPhase::Failed,
                    Some(RestApiDiagnostic::WorkersBlocked),
                    Some(
                        "REST API listener remains bound while all REST workers are blocked."
                            .to_string(),
                    ),
                );
            }
            RestApiDiagnostic::WorkersBlocked
        } else {
            lifecycle
                .status
                .diagnostic
                .unwrap_or(RestApiDiagnostic::ListenerExited)
        };
        drop(lifecycle);
        self.listeners.suspend();
        diagnostic
    }

    pub(crate) fn record_successful_probe(&self) {
        let _transition = self.transition.lock();
        let mut lifecycle = self.lifecycle.lock();
        if lifecycle.listener_owned
            && lifecycle.status.phase == RestApiPhase::Failed
            && lifecycle.status.diagnostic == Some(RestApiDiagnostic::WorkersBlocked)
        {
            let status = self.publish_locked(&mut lifecycle, RestApiPhase::Listening, None, None);
            self.publish_recovery_locked(&mut lifecycle, status);
            drop(lifecycle);
            self.listeners.resume();
        }
    }

    pub(crate) fn status(&self) -> RestApiStatus {
        self.lifecycle.lock().status.clone()
    }

    fn withdraw_listeners(&self, generation: u64) {
        let _transition = self.transition.lock();
        let mut lifecycle = self.lifecycle.lock();
        if lifecycle.status.generation == generation {
            self.listeners.publish(Vec::new());
            if lifecycle.status.phase == RestApiPhase::Listening {
                self.publish_locked(&mut lifecycle, RestApiPhase::Starting, None, None);
            }
        }
    }

    #[cfg(test)]
    fn listener_owned(&self) -> bool {
        self.lifecycle.lock().listener_owned
    }

    pub(crate) fn subscribe_status(&self) -> watch::Receiver<RestApiStatus> {
        self.status_events.subscribe()
    }

    #[cfg(test)]
    fn subscribe_lifecycle(&self) -> broadcast::Receiver<RestApiStatus> {
        self.lifecycle_events.subscribe()
    }

    pub(crate) fn take_recovery_events(&self) -> Option<mpsc::UnboundedReceiver<RestApiRecovery>> {
        self.recovery_receiver.lock().take()
    }

    pub async fn restart_and_wait(&self) -> Result<RestApiStatus, String> {
        let (completed, response) = oneshot::channel();
        self.signal(RestApiCommand::Restart { completed })?;
        tokio::time::timeout(Duration::from_secs(15), response)
            .await
            .map_err(|_| {
                "REST API restart did not complete before the failure deadline".to_string()
            })?
            .map_err(|_| "REST API listener controller stopped".to_string())
    }

    pub(crate) async fn prepare_http_restart(&self) -> Result<RestartAcceptance, String> {
        let (accepted, response) = oneshot::channel();
        let (response_complete, body_complete) = oneshot::channel();
        self.signal(RestApiCommand::PrepareRestart {
            accepted,
            response_complete: body_complete,
        })?;
        let generation = tokio::time::timeout(Duration::from_secs(5), response)
            .await
            .map_err(|_| "Listener supervisor did not acknowledge restart ownership".to_string())?
            .map_err(|_| "Listener supervisor stopped".to_string())??;
        Ok(RestartAcceptance {
            generation,
            response_complete,
        })
    }

    #[cfg(test)]
    async fn shutdown_and_wait(&self) -> Result<(), String> {
        let (completed, response) = oneshot::channel();
        self.signal(RestApiCommand::Shutdown { completed })?;
        tokio::time::timeout(Duration::from_secs(5), response)
            .await
            .map_err(|_| {
                "REST API shutdown did not complete before the failure deadline".to_string()
            })?
            .map_err(|_| "REST API listener controller stopped".to_string())
    }

    #[cfg(test)]
    fn set_restart_barrier(&self, barrier: Arc<tokio::sync::Barrier>) {
        *self.restart_barrier.lock() = Some(barrier);
    }

    #[cfg(test)]
    async fn wait_for_restart_barrier(&self) {
        let barrier = self.restart_barrier.lock().take();
        if let Some(barrier) = barrier {
            barrier.wait().await;
        }
    }

    #[cfg(test)]
    fn set_port_release_barrier(&self, barrier: Arc<tokio::sync::Barrier>) {
        *self.port_release_barrier.lock() = Some(barrier);
    }

    #[cfg(test)]
    async fn wait_for_port_release_barrier(&self) {
        let barrier = self.port_release_barrier.lock().take();
        if let Some(barrier) = barrier {
            barrier.wait().await;
        }
    }

    #[cfg(test)]
    fn install_server_exit(&self) -> oneshot::Sender<()> {
        let (sender, receiver) = oneshot::channel();
        *self.server_exit.lock() = Some(receiver);
        sender
    }

    #[cfg(test)]
    fn take_server_exit(&self) -> Option<oneshot::Receiver<()>> {
        self.server_exit.lock().take()
    }

    #[cfg(test)]
    fn force_shutdown_error(&self) {
        self.shutdown_error.store(true, Ordering::Release);
    }

    #[cfg(test)]
    fn take_shutdown_error(&self) -> bool {
        self.shutdown_error.swap(false, Ordering::AcqRel)
    }

    #[cfg(test)]
    fn force_bind_error(&self) {
        self.bind_error.store(true, Ordering::Release);
    }

    #[cfg(test)]
    fn take_bind_error(&self) -> bool {
        self.bind_error.swap(false, Ordering::AcqRel)
    }

    #[cfg(test)]
    fn install_initial_listener(&self, listener: std::net::TcpListener) {
        *self.initial_listener.lock() = Some(listener);
    }

    #[cfg(test)]
    fn take_initial_listener(&self) -> Option<std::net::TcpListener> {
        self.initial_listener.lock().take()
    }
}

pub(crate) type RestApiControlState = Arc<RestApiControl>;

fn probe_address(address: SocketAddr) -> SocketAddr {
    match address.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => {
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), address.port())
        }
        IpAddr::V6(ip) if ip.is_unspecified() => {
            SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), address.port())
        }
        _ => address,
    }
}

async fn prove_health(address: SocketAddr, deadline: tokio::time::Instant) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .map_err(|error| format!("health proof client for {address} failed: {error}"))?;
    let url = format!("http://{}/api/health", probe_address(address));
    let status = tokio::time::timeout_at(deadline, async {
        let response = client
            .get(url)
            .header(reqwest::header::CONNECTION, "close")
            .send()
            .await?;
        let status = response.status();
        response.bytes().await?;
        Ok::<_, reqwest::Error>(status)
    })
    .await
    .map_err(|_| format!("health proof for {address} exceeded its failure deadline waiting for a complete /api/health response"))?
    .map_err(|error| format!("health proof for {address} failed: {error}"))?;
    if status.is_success() {
        Ok(())
    } else {
        Err(format!("health proof for {address} returned {status}"))
    }
}

async fn bind_listener(bind_addr: &str) -> std::io::Result<std::net::TcpListener> {
    let address = bind_addr
        .parse::<SocketAddr>()
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
    let socket = if address.is_ipv4() {
        tokio::net::TcpSocket::new_v4()?
    } else {
        tokio::net::TcpSocket::new_v6()?
    };
    socket.set_reuseaddr(true)?;
    socket.bind(address)?;
    socket.listen(1024)?.into_std()
}

fn listener_exit_message(result: Result<(), std::io::Error>) -> String {
    match result {
        Ok(()) => "REST API listener/server future exited unexpectedly.".to_string(),
        Err(error) => format!("REST API listener/server future exited unexpectedly: {error}"),
    }
}

async fn await_bind_recovery(
    control: &RestApiControl,
    commands: &mut mpsc::UnboundedReceiver<RestApiCommand>,
) -> Option<GenerationEvent> {
    loop {
        match commands.recv().await {
            Some(RestApiCommand::PortReleased) => return Some(GenerationEvent::Replace),
            Some(RestApiCommand::Restart { completed }) => {
                if let Some(event) = native_restart(control, completed) {
                    return Some(event);
                }
            }
            Some(RestApiCommand::PrepareRestart {
                accepted,
                response_complete,
            }) => {
                restart::accept(control, accepted, response_complete);
            }
            Some(RestApiCommand::ReservedRestart {
                generation,
                completed,
            }) => {
                if let Some(event) = reserved_restart(control, generation, completed) {
                    return Some(event);
                }
            }
            #[cfg(test)]
            Some(RestApiCommand::Shutdown { completed }) => {
                return Some(GenerationEvent::Shutdown(completed))
            }
            None => {
                *control.sender.lock() = None;
                return None;
            }
        }
    }
}

fn native_restart(
    control: &RestApiControl,
    completed: oneshot::Sender<RestApiStatus>,
) -> Option<GenerationEvent> {
    let lifecycle = control.lifecycle.lock();
    if lifecycle.pending_restart.is_some() {
        let mut rejected = lifecycle.status.clone();
        rejected.phase = RestApiPhase::Failed;
        rejected.message = Some("Listener restart is already reserved".into());
        let _ = completed.send(rejected);
        None
    } else {
        Some(GenerationEvent::Restart(completed))
    }
}

fn reserved_restart(
    control: &RestApiControl,
    generation: u64,
    completed: oneshot::Sender<RestApiStatus>,
) -> Option<GenerationEvent> {
    let status = control.status();
    if status.generation.checked_add(1) == Some(generation)
        && control.lifecycle.lock().pending_restart == Some(generation)
    {
        return Some(GenerationEvent::Restart(completed));
    }
    let _ = completed.send(status);
    None
}

async fn finish_generation<F>(
    control: &RestApiControl,
    generation: u64,
    shutdown_tx: oneshot::Sender<()>,
    server: F,
    streams: &streams::GenerationStreams,
    event: GenerationEvent,
) -> Option<GenerationEvent>
where
    F: std::future::Future<Output = Result<(), std::io::Error>>,
{
    control.withdraw_listeners(generation);
    streams.close();
    if matches!(event, GenerationEvent::ServerExited) {
        return Some(GenerationEvent::Replace);
    }
    let _ = shutdown_tx.send(());
    #[cfg(test)]
    let shutdown_result = if control.take_shutdown_error() {
        Err("forced shutdown failure".to_string())
    } else {
        drain_server(server, Duration::from_secs(5)).await
    };
    #[cfg(not(test))]
    let shutdown_result = drain_server(server, Duration::from_secs(5)).await;
    if let Err(error) = shutdown_result {
        let message = format!("REST API listener/server shutdown failed: {error}");
        log::error!("{message}");
        let failed = control.fail_generation(
            generation,
            RestApiDiagnostic::ListenerExited,
            message,
            false,
        );
        if let GenerationEvent::Restart(completed) = event {
            if let Some(failed) = failed {
                let _ = completed.send(failed);
            }
            return Some(GenerationEvent::Replace);
        }
    }
    Some(event)
}

async fn drain_server<F: std::future::Future<Output = Result<(), std::io::Error>>>(
    server: F,
    failure_deadline: Duration,
) -> Result<(), String> {
    tokio::time::timeout(failure_deadline, server)
        .await
        .map_err(|_| "graceful completion exceeded its failure deadline".to_string())?
        .map_err(|error| error.to_string())
}

async fn await_running_event<S, T>(
    control: &RestApiControl,
    generation: u64,
    commands: &mut mpsc::UnboundedReceiver<RestApiCommand>,
    mut server: std::pin::Pin<&mut S>,
    mut test_server_exit: std::pin::Pin<&mut T>,
    test_server_exit_pending: &mut bool,
) -> Option<GenerationEvent>
where
    S: std::future::Future<Output = Result<(), std::io::Error>>,
    T: std::future::Future<Output = ()>,
{
    loop {
        tokio::select! {
            result = &mut server => {
                let message = listener_exit_message(result);
                log::error!("{message}");
                control.fail_generation(
                    generation,
                    RestApiDiagnostic::ListenerExited,
                    message,
                    false,
                );
                return Some(GenerationEvent::ServerExited);
            }
            _ = &mut test_server_exit, if *test_server_exit_pending => {
                *test_server_exit_pending = false;
                let message = "REST API listener/server future exited unexpectedly.".to_string();
                log::error!("{message}");
                control.fail_generation(
                    generation,
                    RestApiDiagnostic::ListenerExited,
                    message,
                    false,
                );
                return Some(GenerationEvent::Replace);
            }
            command = commands.recv() => {
                match command {
                    Some(RestApiCommand::PortReleased) => {
                        #[cfg(test)]
                        control.wait_for_port_release_barrier().await;
                    }
                    Some(RestApiCommand::Restart { completed }) => {
                        #[cfg(test)]
                        control.wait_for_restart_barrier().await;
                        if let Some(event) = native_restart(control, completed) { return Some(event); }
                    }
                    Some(RestApiCommand::PrepareRestart { accepted, response_complete }) => restart::accept(control, accepted, response_complete),
                    Some(RestApiCommand::ReservedRestart { generation, completed }) => {
                        if let Some(event) = reserved_restart(control, generation, completed) { return Some(event); }
                    },
                    #[cfg(test)]
                    Some(RestApiCommand::Shutdown { completed }) => {
                        return Some(GenerationEvent::Shutdown(completed));
                    }
                    None => return None,
                }
            }
        }
    }
}

pub(crate) fn spawn(
    control: RestApiControlState,
    bind_addr: String,
    router: axum::Router,
) -> std::thread::JoinHandle<()> {
    codeterm_util::named_thread::spawn("rest-api", move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .thread_name("rest-api-worker")
            .worker_threads(4)
            .enable_all()
            .build()
            .expect("Failed to create Tokio runtime for REST API");
        runtime.block_on(start(control, bind_addr, router));
    })
}

pub(crate) async fn start(control: RestApiControlState, bind_addr: String, router: axum::Router) {
    let (sender, mut commands) = mpsc::unbounded_channel();
    control.install(sender);
    #[cfg(test)]
    let test_server_exit = control.take_server_exit();
    #[cfg(test)]
    let mut test_server_exit = std::pin::pin!(async move {
        match test_server_exit {
            Some(receiver) => {
                let _ = receiver.await;
            }
            None => std::future::pending::<()>().await,
        }
    });
    #[cfg(not(test))]
    let mut test_server_exit = std::pin::pin!(std::future::pending::<()>());
    #[cfg(test)]
    let mut test_server_exit_pending = true;
    #[cfg(not(test))]
    let mut test_server_exit_pending = false;
    let mut pending_completions: VecDeque<oneshot::Sender<RestApiStatus>> = VecDeque::new();

    loop {
        let generation = control.begin_generation();
        #[cfg(test)]
        let listener = if control.take_bind_error() {
            Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                "forced bind ownership failure",
            ))
        } else if let Some(listener) = control.take_initial_listener() {
            Ok(listener)
        } else {
            bind_listener(&bind_addr).await
        };
        #[cfg(not(test))]
        let listener = bind_listener(&bind_addr).await;
        let listener = match listener {
            Ok(primary) => listeners::bind_remote(primary, control.listeners.remote_bind()).await,
            Err(error) => Err(error),
        };
        let (listener, remote_listener) = match listener {
            Ok(listener) => listener,
            Err(error) => {
                let message =
                    format!("REST API bind ownership failed for {bind_addr}: {error}. Release the port to retry.");
                log::error!("{message}");
                let failed = control
                    .fail_generation(
                        generation,
                        RestApiDiagnostic::BindOwnershipFailed,
                        message,
                        false,
                    )
                    .expect("active REST generation must accept its bind failure");
                if let Some(completed) = pending_completions.pop_front() {
                    let _ = completed.send(failed);
                }
                if !pending_completions.is_empty() {
                    continue;
                }
                match await_bind_recovery(&control, &mut commands).await {
                    Some(GenerationEvent::Replace) => continue,
                    Some(GenerationEvent::ServerExited) => {
                        unreachable!("bind recovery cannot observe a server exit")
                    }
                    Some(GenerationEvent::Restart(completed)) => {
                        pending_completions.push_back(completed);
                        continue;
                    }
                    #[cfg(test)]
                    Some(GenerationEvent::Shutdown(completed)) => {
                        let _ = completed.send(());
                        return;
                    }
                    None => return,
                }
            }
        };

        let local_addr = match listener.local_addr() {
            Ok(address) => address,
            Err(error) => {
                let message =
                    format!("REST API could not inspect bound socket {bind_addr}: {error}");
                log::error!("{message}");
                let failed = control
                    .fail_generation(
                        generation,
                        RestApiDiagnostic::BindOwnershipFailed,
                        message,
                        false,
                    )
                    .expect("active REST generation must accept its socket inspection failure");
                if let Some(completed) = pending_completions.pop_front() {
                    let _ = completed.send(failed);
                }
                if !pending_completions.is_empty() {
                    continue;
                }
                match await_bind_recovery(&control, &mut commands).await {
                    Some(GenerationEvent::Replace) => continue,
                    Some(GenerationEvent::ServerExited) => {
                        unreachable!("bind recovery cannot observe a server exit")
                    }
                    Some(GenerationEvent::Restart(completed)) => {
                        pending_completions.push_back(completed);
                        continue;
                    }
                    #[cfg(test)]
                    Some(GenerationEvent::Shutdown(completed)) => {
                        let _ = completed.send(());
                        return;
                    }
                    None => return,
                }
            }
        };
        control.listener_bound(generation, local_addr);
        let remote_addr = remote_listener
            .as_ref()
            .map(|listener| listener.local_addr())
            .transpose()
            .expect("successfully bound tailnet listener has an address");
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let streams = streams::GenerationStreams::new();
        let generation_router = streams.router(router.clone());
        let primary = listeners::serve_listener(listener, generation_router.clone(), shutdown_rx);
        let mut server = Box::pin(listeners::serve(
            primary,
            remote_listener,
            generation_router,
            control.listeners.clone(),
        ));
        let health = listeners::prove_set(local_addr, remote_addr);
        tokio::pin!(health);

        let event = loop {
            tokio::select! {
                result = &mut server => {
                    let message = listener_exit_message(result);
                    log::error!("{message}");
                    control.fail_generation(
                        generation,
                        RestApiDiagnostic::ListenerExited,
                        message,
                        false,
                    );
                    break GenerationEvent::ServerExited;
                }
                _ = &mut test_server_exit, if test_server_exit_pending => {
                    #[cfg(test)]
                    {
                        test_server_exit_pending = false;
                    }
                    let message = "REST API listener/server future exited unexpectedly.".to_string();
                    log::error!("{message}");
                    control.fail_generation(
                        generation,
                        RestApiDiagnostic::ListenerExited,
                        message,
                        false,
                    );
                    break GenerationEvent::Replace;
                }
                proof = &mut health => {
                    let restart_queued = match proof {
                        Ok(()) => {
                            let listening = control
                                .mark_listening(generation, std::iter::once(local_addr).chain(remote_addr).collect())
                                .expect("health proof belongs to the active bound generation");
                            if let Some(completed) = pending_completions.pop_front() {
                                let _ = completed.send(listening);
                            }
                            log::info!(
                                "REST API generation {generation} health-proven on http://{bind_addr}"
                            );
                            !pending_completions.is_empty()
                        }
                        Err(error) => {
                            let message = format!(
                                "REST API listener remains bound but /api/health proof failed: {error}"
                            );
                            log::error!("{message}");
                            let failed = control
                                .fail_generation(
                                    generation,
                                    RestApiDiagnostic::WorkersBlocked,
                                    message,
                                    true,
                                )
                                .expect("health proof belongs to the active bound generation");
                            if let Some(completed) = pending_completions.pop_front() {
                                let _ = completed.send(failed);
                            }
                            !pending_completions.is_empty()
                        }
                    };
                    if restart_queued {
                        break GenerationEvent::Replace;
                    }
                    break match await_running_event(
                        &control,
                        generation,
                        &mut commands,
                        server.as_mut(),
                        test_server_exit.as_mut(),
                        &mut test_server_exit_pending,
                    )
                    .await
                    {
                        Some(event) => event,
                        None => return,
                    };
                }
                command = commands.recv() => {
                    match command {
                        Some(RestApiCommand::PortReleased) => {
                            #[cfg(test)]
                            control.wait_for_port_release_barrier().await;
                        }
                        Some(RestApiCommand::Restart { completed }) => {
                            #[cfg(test)]
                            control.wait_for_restart_barrier().await;
                            if let Some(event) = native_restart(&control, completed) { break event; }
                        }
                        Some(RestApiCommand::PrepareRestart { accepted, response_complete }) => restart::accept(&control, accepted, response_complete),
                        Some(RestApiCommand::ReservedRestart { generation, completed }) => {
                            if let Some(event) = reserved_restart(&control, generation, completed) { break event; }
                        },
                        #[cfg(test)]
                        Some(RestApiCommand::Shutdown { completed }) => {
                            break GenerationEvent::Shutdown(completed);
                        }
                        None => return,
                    }
                }
            }
        };

        let event =
            match finish_generation(&control, generation, shutdown_tx, server, &streams, event)
                .await
            {
                Some(event) => event,
                None => return,
            };
        match event {
            GenerationEvent::Replace => {}
            GenerationEvent::ServerExited => unreachable!("server exit is normalized"),
            GenerationEvent::Restart(completed) => pending_completions.push_back(completed),
            #[cfg(test)]
            GenerationEvent::Shutdown(completed) => {
                let _ = completed.send(());
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests;
