mod notifications;
#[cfg(test)]
mod tests;

use super::{annotate_list, runtime::callable::Calls, same_session, storage::Metadata};
use crate::service::{account::AccountCell, ServiceContext};
use codeterm_neighbors::{
    adapter::FirebaseTransport, registry::RegistrySignal, supervisor::ListenerState, Error, Result,
};
use serde_json::json;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tokio::sync::{watch, Notify};

#[derive(Default)]
struct UpdateCell {
    generation: AtomicU64,
    task: parking_lot::Mutex<Option<Handle>>,
}

struct Handle {
    task: tokio::task::JoinHandle<()>,
    stopped: watch::Sender<bool>,
    retry: Arc<Notify>,
}

impl ServiceContext {
    fn neighbor_updates(&self) -> &UpdateCell {
        self.extensions.get_or_init(UpdateCell::default)
    }

    pub(crate) fn restore_neighbor_updates_once(&self) {
        if cfg!(any(test, feature = "test-support")) {
            return;
        }
        if self.require_neighbors_enabled().is_err() {
            return;
        }
        let Some(account) = self.ext::<AccountCell>().0.get().cloned() else {
            return;
        };
        let cell = self.neighbor_updates();
        let mut slot = cell.task.lock();
        if let Some(handle) = slot.as_ref().filter(|handle| !handle.task.is_finished()) {
            handle.retry.notify_one();
            return;
        }
        let generation = cell.generation.load(Ordering::Acquire);
        let (stopped, receiver) = watch::channel(false);
        let retry = Arc::new(Notify::new());
        let service = self.clone();
        let retry_task = retry.clone();
        let task = tokio::spawn(async move {
            let _ = run(service, account, generation, receiver, retry_task).await;
        });
        *slot = Some(Handle {
            task,
            stopped,
            retry,
        });
    }

    pub(super) fn stop_neighbor_updates(&self) {
        let cell = self.neighbor_updates();
        let mut slot = cell.task.lock();
        cell.generation.fetch_add(1, Ordering::AcqRel);
        if let Some(handle) = slot.take() {
            let _ = handle.stopped.send(true);
            handle.task.abort();
        }
    }
}

struct Session {
    service: ServiceContext,
    account: Arc<codeterm_account::Account>,
    expected: codeterm_account::Session,
    generation: u64,
    transport: FirebaseTransport,
    metadata: Metadata,
}

impl Session {
    fn current(&self) -> Result<()> {
        self.service.require_neighbors_enabled()?;
        if self
            .service
            .neighbor_updates()
            .generation
            .load(Ordering::Acquire)
            != self.generation
        {
            return Err(Error::Revoked);
        }
        Ok(())
    }

    fn state(&self, status: ListenerState) {
        if self.current().is_ok() {
            self.service.ui.emit(
                "neighbors-watch-status",
                json!({"uid":self.transport.uid,"status":status}),
            );
        }
    }

    async fn refresh(&self, signal: RegistrySignal, seen: &mut notifications::Seen) -> Result<()> {
        self.current()?;
        let mut list = self
            .transport
            .calls
            .invoke("friendsList", json!({}))
            .await?;
        annotate_list(&mut list, &self.metadata.settings().await?).map_err(|_| Error::Transport)?;
        let notices = notifications::changes(seen, &list, &signal.accepted)?;
        self.metadata
            .write(
                "registry-notifications.json".into(),
                serde_json::to_value(&*seen).map_err(|_| Error::CursorStore)?,
            )
            .await?;
        if !same_session(self.account.session().await.as_ref(), &self.expected) {
            return Err(Error::Revoked);
        }
        self.current()?;
        let service = self.service.clone();
        let uid = self.transport.uid.clone();
        let generation = self.generation;
        tokio::task::spawn_blocking(move || {
            if service.require_neighbors_enabled().is_err()
                || service
                    .neighbor_updates()
                    .generation
                    .load(Ordering::Acquire)
                    != generation
            {
                return;
            }
            service
                .ui
                .emit("neighbors-changed", json!({"uid":uid,"list":list}));
            for notice in notices {
                if service.require_neighbors_enabled().is_err() {
                    break;
                }
                service.create_notification_actions_reported(
                    notice.title,
                    Some("Open Neighborhood to view this connection.".into()),
                    Some("info"),
                    crate::notification::store::Ask::default(),
                    Some(false),
                    crate::service::notification::NotificationOrigin::default(),
                    crate::service::notification::PushIntent {
                        level: Some(crate::notification::push::NotifyLevel::Verbose),
                        event_key: Some(format!(
                            "neighbor:{}:{}",
                            codeterm_neighbors::crypto::sha256_hex(uid.as_bytes()),
                            notice.event_id
                        )),
                    },
                    None,
                );
            }
        })
        .await
        .map_err(|_| Error::Transport)?;
        Ok(())
    }
}

async fn stopped(receiver: &mut watch::Receiver<bool>) {
    if *receiver.borrow_and_update() {
        return;
    }
    let _ = receiver.changed().await;
}

async fn run(
    service: ServiceContext,
    account: Arc<codeterm_account::Account>,
    generation: u64,
    mut shutdown: watch::Receiver<bool>,
    retry: Arc<Notify>,
) -> Result<()> {
    let expected = account
        .fresh_session()
        .await
        .map_err(|_| Error::Transport)?
        .ok_or(Error::Revoked)?;
    if expected.identity.provider == "local" {
        return Ok(());
    }
    let endpoints = codeterm_account::firebase_endpoints::current();
    if endpoints.emulator {
        return Ok(());
    }
    let uid = expected.identity.uid.clone();
    let metadata = Metadata::new(
        service
            .account_data_root()
            .map_err(|_| Error::CursorStore)?,
        &uid,
    );
    let transport = FirebaseTransport {
        project: if endpoints.qa {
            "domios-signin-qa"
        } else {
            "domios-d9357"
        }
        .into(),
        uid: uid.clone(),
        calls: Arc::new(Calls {
            service: service.clone(),
            uid,
            generation: expected.login_generation,
        }),
    };
    let session = Session {
        service,
        account,
        expected,
        generation,
        transport,
        metadata,
    };
    let mut seen = session
        .metadata
        .read("registry-notifications.json".into())
        .await?
        .unwrap_or_default();
    for attempt in 0..=3 {
        if attempt > 0 {
            session.state(ListenerState::Reconnecting);
            tokio::select! {
                biased;
                _ = stopped(&mut shutdown) => return Err(Error::Revoked),
                _ = retry.notified() => {}
            }
        }
        session.current()?;
        session.state(ListenerState::Connecting);
        let result = async {
            let mut listener = session.transport.listen_registry().await?;
            loop {
                let signal = listener.next().await?.ok_or(Error::Transport)?;
                session.refresh(signal, &mut seen).await?;
                session.state(ListenerState::Receiving);
            }
        };
        let outcome = tokio::select! {
            biased;
            _ = stopped(&mut shutdown) => Err(Error::Revoked),
            result = result => result,
        };
        match outcome {
            Err(Error::Transport) if attempt < 3 => {}
            outcome => {
                session.state(ListenerState::Stopped);
                return outcome;
            }
        }
    }
    Err(Error::Transport)
}
