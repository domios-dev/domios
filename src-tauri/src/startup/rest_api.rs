use std::sync::Arc;

use parking_lot::Mutex;
use tokio::sync::{mpsc, oneshot, watch};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RestApiStatus {
    Starting,
    Listening,
    Failed(String),
}

#[derive(Clone)]
pub(crate) struct RestApiControl {
    sender: Arc<Mutex<Option<mpsc::UnboundedSender<RestApiCommand>>>>,
    status_events: watch::Sender<RestApiStatus>,
}

#[derive(Clone, Copy, Debug)]
enum RestApiCommand {
    PortReleased,
    Restart,
}

impl RestApiControl {
    pub(crate) fn new() -> Self {
        let (status_events, _) = watch::channel(RestApiStatus::Starting);
        Self {
            sender: Arc::new(Mutex::new(None)),
            status_events,
        }
    }

    pub(crate) fn port_released(&self) -> Result<(), String> {
        self.signal(RestApiCommand::PortReleased)
    }

    pub(crate) fn restart(&self) -> Result<(), String> {
        self.signal(RestApiCommand::Restart)
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

    fn set_status(&self, status: RestApiStatus) {
        self.status_events.send_replace(status);
    }

    #[cfg(test)]
    fn subscribe_status(&self) -> watch::Receiver<RestApiStatus> {
        self.status_events.subscribe()
    }
}

pub(crate) type RestApiControlState = Arc<RestApiControl>;

pub(crate) async fn start(control: RestApiControlState, bind_addr: String, router: axum::Router) {
    let (sender, mut commands) = mpsc::unbounded_channel();
    control.install(sender);
    loop {
        match tokio::net::TcpListener::bind(&bind_addr).await {
            Ok(listener) => {
                control.set_status(RestApiStatus::Listening);
                log::info!("REST API listening on http://{bind_addr}");
                let (shutdown_tx, shutdown_rx) = oneshot::channel();
                tokio::select! {
                    result = axum::serve(listener, router.clone()).with_graceful_shutdown(async { let _ = shutdown_rx.await; }) => {
                        if let Err(error) = result { log::error!("REST API server error: {error}"); }
                        return;
                    }
                    command = commands.recv() => {
                        let Some(command) = command else { return; };
                        if matches!(command, RestApiCommand::Restart) {
                            control.set_status(RestApiStatus::Starting);
                            let _ = shutdown_tx.send(());
                        }
                    }
                }
            }
            Err(error) => {
                let message = format!("REST API could not bind {bind_addr}: {error}. Release the port, then use Restart REST API.");
                control.set_status(RestApiStatus::Failed(message.clone()));
                log::error!("{message}");
                match commands.recv().await {
                    Some(RestApiCommand::PortReleased | RestApiCommand::Restart) => continue,
                    None => return,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn wait_for_status(
        status: &mut watch::Receiver<RestApiStatus>,
        expected: fn(&RestApiStatus) -> bool,
    ) -> RestApiStatus {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
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

    #[tokio::test]
    async fn occupied_port_release_event_rebinds_the_real_listener() {
        let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = occupied.local_addr().unwrap();
        let control = Arc::new(RestApiControl::new());
        let mut status = control.subscribe_status();
        let router = axum::Router::new().route("/health", axum::routing::get(|| async { "ok" }));
        let task = tokio::spawn(start(control.clone(), address.to_string(), router));
        let failed = wait_for_status(&mut status, |s| matches!(s, RestApiStatus::Failed(_))).await;
        assert!(
            matches!(failed, RestApiStatus::Failed(message) if message.contains("Release the port"))
        );
        drop(occupied);
        control.port_released().unwrap();
        wait_for_status(&mut status, |s| matches!(s, RestApiStatus::Listening)).await;
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
        wait_for_status(&mut status, |s| matches!(s, RestApiStatus::Listening)).await;
        control.restart().unwrap();
        wait_for_status(&mut status, |s| matches!(s, RestApiStatus::Listening)).await;
        let stream = tokio::net::TcpStream::connect(address).await.unwrap();
        drop(stream);
        task.abort();
    }
}
