use std::{future::IntoFuture, sync::Arc};

use parking_lot::Mutex;
use serde::Serialize;
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::{mpsc, oneshot, watch};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RestApiPhase {
    Starting,
    Listening,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct RestApiStatus {
    pub(crate) generation: u64,
    pub(crate) phase: RestApiPhase,
    pub(crate) message: Option<String>,
}

#[derive(Clone)]
pub(crate) struct RestApiControl {
    sender: Arc<Mutex<Option<mpsc::UnboundedSender<RestApiCommand>>>>,
    status_events: watch::Sender<RestApiStatus>,
    #[cfg(test)]
    restart_barrier: Arc<Mutex<Option<Arc<tokio::sync::Barrier>>>>,
    #[cfg(test)]
    port_release_barrier: Arc<Mutex<Option<Arc<tokio::sync::Barrier>>>>,
    #[cfg(test)]
    server_exit: Arc<Mutex<Option<oneshot::Receiver<()>>>>,
    #[cfg(test)]
    shutdown_error: Arc<AtomicBool>,
}

#[derive(Debug)]
enum RestApiCommand {
    PortReleased,
    Restart {
        completed: oneshot::Sender<RestApiStatus>,
    },
}

impl RestApiControl {
    pub(crate) fn new() -> Self {
        let (status_events, _) = watch::channel(RestApiStatus {
            generation: 0,
            phase: RestApiPhase::Starting,
            message: None,
        });
        Self {
            sender: Arc::new(Mutex::new(None)),
            status_events,
            #[cfg(test)]
            restart_barrier: Arc::new(Mutex::new(None)),
            #[cfg(test)]
            port_release_barrier: Arc::new(Mutex::new(None)),
            #[cfg(test)]
            server_exit: Arc::new(Mutex::new(None)),
            #[cfg(test)]
            shutdown_error: Arc::new(AtomicBool::new(false)),
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
            .map_err(|_| "REST API listener controller stopped; restart CodeTerm".to_string())
    }

    fn install(&self, sender: mpsc::UnboundedSender<RestApiCommand>) {
        *self.sender.lock() = Some(sender);
    }

    fn set_status(&self, phase: RestApiPhase, message: Option<String>) -> RestApiStatus {
        let generation = self.status_events.borrow().generation + 1;
        let status = RestApiStatus {
            generation,
            phase,
            message,
        };
        self.status_events.send_replace(status.clone());
        status
    }

    fn fail_pending(
        &self,
        pending_completion: &mut Option<oneshot::Sender<RestApiStatus>>,
        message: String,
    ) {
        let failed = self.set_status(RestApiPhase::Failed, Some(message));
        if let Some(completed) = pending_completion.take() {
            let _ = completed.send(failed);
        }
    }

    pub(crate) fn status(&self) -> RestApiStatus {
        self.status_events.borrow().clone()
    }

    #[cfg(test)]
    fn subscribe_status(&self) -> watch::Receiver<RestApiStatus> {
        self.status_events.subscribe()
    }

    pub(crate) async fn restart_and_wait(&self) -> Result<RestApiStatus, String> {
        let (completed, response) = oneshot::channel();
        self.signal(RestApiCommand::Restart { completed })?;
        tokio::time::timeout(std::time::Duration::from_secs(5), response)
            .await
            .map_err(|_| {
                "REST API restart did not complete before the failure deadline".to_string()
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
}

pub(crate) type RestApiControlState = Arc<RestApiControl>;

async fn wait_for_recovery_command(
    commands: &mut mpsc::UnboundedReceiver<RestApiCommand>,
    pending_completion: &mut Option<oneshot::Sender<RestApiStatus>>,
) -> bool {
    match commands.recv().await {
        Some(RestApiCommand::PortReleased) => true,
        Some(RestApiCommand::Restart { completed }) => {
            *pending_completion = Some(completed);
            true
        }
        None => false,
    }
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
    let test_server_exit_pending = false;
    let mut pending_completion: Option<oneshot::Sender<RestApiStatus>> = None;
    loop {
        match tokio::net::TcpListener::bind(&bind_addr).await {
            Ok(listener) => {
                let listening = control.set_status(RestApiPhase::Listening, None);
                if let Some(completed) = pending_completion.take() {
                    let _ = completed.send(listening);
                }
                log::info!("REST API listening on http://{bind_addr}");
                let (shutdown_tx, shutdown_rx) = oneshot::channel();
                let server = axum::serve(listener, router.clone())
                    .with_graceful_shutdown(async {
                        let _ = shutdown_rx.await;
                    })
                    .into_future();
                tokio::pin!(server);
                let command = loop {
                    tokio::select! {
                        result = &mut server => {
                            let message = match result {
                                Ok(()) => "REST API server exited unexpectedly. Use Restart REST API to restore the listener.".to_string(),
                                Err(error) => format!("REST API server stopped unexpectedly: {error}. Use Restart REST API to restore the listener."),
                            };
                            break Err(message);
                        }
                        _ = &mut test_server_exit, if test_server_exit_pending => {
                            #[cfg(test)]
                            { test_server_exit_pending = false; }
                            let message = "REST API server exited unexpectedly. Use Restart REST API to restore the listener.".to_string();
                            break Err(message);
                        }
                        command = commands.recv() => {
                            let Some(command) = command else { return; };
                            match command {
                                RestApiCommand::PortReleased => {
                                    #[cfg(test)]
                                    control.wait_for_port_release_barrier().await;
                                }
                                RestApiCommand::Restart { completed } => break Ok(completed),
                            }
                        }
                    }
                };
                let command = match command {
                    Ok(command) => command,
                    Err(message) => {
                        log::error!("{message}");
                        control.fail_pending(&mut pending_completion, message);
                        if wait_for_recovery_command(&mut commands, &mut pending_completion).await {
                            continue;
                        }
                        return;
                    }
                };
                control.set_status(RestApiPhase::Starting, None);
                #[cfg(test)]
                control.wait_for_restart_barrier().await;
                pending_completion = Some(command);
                let _ = shutdown_tx.send(());
                #[cfg(test)]
                let shutdown_result = if control.take_shutdown_error() {
                    Err("forced shutdown failure".to_string())
                } else {
                    server.await.map_err(|error| error.to_string())
                };
                #[cfg(not(test))]
                let shutdown_result = server.await.map_err(|error| error.to_string());
                if let Err(error) = shutdown_result {
                    let message = format!("REST API server shutdown failed: {error}. Use Restart REST API to restore the listener.");
                    log::error!("{message}");
                    control.fail_pending(&mut pending_completion, message);
                    if wait_for_recovery_command(&mut commands, &mut pending_completion).await {
                        continue;
                    }
                    return;
                }
            }
            Err(error) => {
                let message = format!("REST API could not bind {bind_addr}: {error}. Release the port, then use Restart REST API.");
                control.fail_pending(&mut pending_completion, message.clone());
                log::error!("{message}");
                if wait_for_recovery_command(&mut commands, &mut pending_completion).await {
                    continue;
                }
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn wait_for_status<F>(
        status: &mut watch::Receiver<RestApiStatus>,
        expected: F,
    ) -> RestApiStatus
    where
        F: Fn(&RestApiStatus) -> bool,
    {
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            loop {
                let current = status.borrow_and_update().clone();
                if expected(&current) {
                    return current;
                }
                status.changed().await.unwrap();
            }
        })
        .await
        .expect("listener lifecycle event deadline elapsed")
    }

    fn assert_recoverable_failure(status: &RestApiStatus, detail: &str) {
        let message = status
            .message
            .as_deref()
            .expect("failure must explain recovery");
        assert!(message.contains(detail));
        assert!(message.contains("Restart REST API"));
        assert!(!message.contains("Restart CodeTerm"));
    }

    #[tokio::test]
    async fn occupied_port_release_event_rebinds_the_real_listener() {
        let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = occupied.local_addr().unwrap();
        let control = Arc::new(RestApiControl::new());
        let mut status = control.subscribe_status();
        let router = axum::Router::new().route("/health", axum::routing::get(|| async { "ok" }));
        let task = tokio::spawn(start(control.clone(), address.to_string(), router));
        let failed = wait_for_status(&mut status, |s| s.phase == RestApiPhase::Failed).await;
        assert!(failed
            .message
            .as_deref()
            .is_some_and(|message| message.contains("Release the port")));
        drop(occupied);
        control.port_released().unwrap();
        wait_for_status(&mut status, |s| s.phase == RestApiPhase::Listening).await;
        let response = reqwest::get(format!("http://{address}/health"))
            .await
            .unwrap();
        assert_eq!(response.text().await.unwrap(), "ok");
        task.abort();
    }

    #[tokio::test]
    async fn manual_restart_rebinds_a_real_listener() {
        let address = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let control = Arc::new(RestApiControl::new());
        let mut status = control.subscribe_status();
        let task = tokio::spawn(start(
            control.clone(),
            address.to_string(),
            axum::Router::new(),
        ));
        let initial = wait_for_status(&mut status, |s| s.phase == RestApiPhase::Listening).await;
        let returned = control.restart_and_wait().await.unwrap();
        assert_eq!(returned.phase, RestApiPhase::Listening);
        assert!(returned.generation > initial.generation);
        let stream = tokio::net::TcpStream::connect(address).await.unwrap();
        drop(stream);
        task.abort();
    }

    #[tokio::test]
    async fn port_release_while_listening_keeps_the_real_listener_running() {
        let address = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let control = Arc::new(RestApiControl::new());
        let mut status = control.subscribe_status();
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        control.set_port_release_barrier(barrier.clone());
        let router = axum::Router::new().route("/health", axum::routing::get(|| async { "ok" }));
        let task = tokio::spawn(start(control.clone(), address.to_string(), router));
        let listening = wait_for_status(&mut status, |s| s.phase == RestApiPhase::Listening).await;

        control.port_released().unwrap();
        barrier.wait().await;

        assert_eq!(control.status(), listening);
        let response = reqwest::get(format!("http://{address}/health"))
            .await
            .unwrap();
        assert_eq!(response.text().await.unwrap(), "ok");
        task.abort();
    }

    #[tokio::test]
    async fn unexpected_server_exit_recovers_on_a_later_restart() {
        let address = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let control = Arc::new(RestApiControl::new());
        let exit = control.install_server_exit();
        let mut status = control.subscribe_status();
        let task = tokio::spawn(start(
            control.clone(),
            address.to_string(),
            axum::Router::new().route("/health", axum::routing::get(|| async { "ok" })),
        ));
        wait_for_status(&mut status, |s| s.phase == RestApiPhase::Listening).await;

        exit.send(()).unwrap();
        let failed = wait_for_status(&mut status, |s| s.phase == RestApiPhase::Failed).await;
        assert_recoverable_failure(&failed, "exited unexpectedly");
        let recovered = control.restart_and_wait().await.unwrap();
        assert_eq!(recovered.phase, RestApiPhase::Listening);
        assert!(recovered.generation > failed.generation);
        let response = reqwest::get(format!("http://{address}/health"))
            .await
            .unwrap();
        assert_eq!(response.text().await.unwrap(), "ok");
        task.abort();
    }

    #[tokio::test]
    async fn shutdown_error_fails_its_restart_completion_then_recovers() {
        let address = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let control = Arc::new(RestApiControl::new());
        control.force_shutdown_error();
        let mut status = control.subscribe_status();
        let task = tokio::spawn(start(
            control.clone(),
            address.to_string(),
            axum::Router::new().route("/health", axum::routing::get(|| async { "ok" })),
        ));
        wait_for_status(&mut status, |s| s.phase == RestApiPhase::Listening).await;

        let restart_control = control.clone();
        let restart =
            tokio::spawn(async move { restart_control.restart_and_wait().await.unwrap() });
        let failed = wait_for_status(&mut status, |s| s.phase == RestApiPhase::Failed).await;
        let completed = restart.await.unwrap();
        assert_eq!(completed, failed);
        assert_recoverable_failure(&completed, "shutdown failed");
        let recovered = control.restart_and_wait().await.unwrap();
        assert_eq!(recovered.phase, RestApiPhase::Listening);
        assert!(recovered.generation > completed.generation);
        let response = reqwest::get(format!("http://{address}/health"))
            .await
            .unwrap();
        assert_eq!(response.text().await.unwrap(), "ok");
        task.abort();
    }

    #[tokio::test]
    async fn overlapping_restarts_complete_from_their_own_listener_generations() {
        let address = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        let control = Arc::new(RestApiControl::new());
        let mut status = control.subscribe_status();
        let barrier = Arc::new(tokio::sync::Barrier::new(2));
        control.set_restart_barrier(barrier.clone());
        let task = tokio::spawn(start(
            control.clone(),
            address.to_string(),
            axum::Router::new(),
        ));
        wait_for_status(&mut status, |s| s.phase == RestApiPhase::Listening).await;

        let first_control = control.clone();
        let first = tokio::spawn(async move { first_control.restart_and_wait().await.unwrap() });
        wait_for_status(&mut status, |s| s.phase == RestApiPhase::Starting).await;

        let second_control = control.clone();
        let second = tokio::spawn(async move { second_control.restart_and_wait().await.unwrap() });
        barrier.wait().await;

        let first = first.await.unwrap();
        let second = second.await.unwrap();
        assert_eq!(first.phase, RestApiPhase::Listening);
        assert_eq!(second.phase, RestApiPhase::Listening);
        assert!(second.generation > first.generation);
        task.abort();
    }

    #[test]
    fn status_read_reports_generation_phase_and_failure_message() {
        let control = RestApiControl::new();
        control.set_status(RestApiPhase::Failed, Some("port is occupied".into()));
        assert_eq!(
            control.status(),
            RestApiStatus {
                generation: 1,
                phase: RestApiPhase::Failed,
                message: Some("port is occupied".into()),
            }
        );
    }
}
