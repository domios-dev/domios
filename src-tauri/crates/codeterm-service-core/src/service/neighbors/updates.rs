mod delivery;
mod notifications;
mod read;
#[cfg(test)]
mod tests;

use super::{annotate_list, runtime::callable::Calls, same_session, storage::Metadata};
use crate::service::{account::AccountCell, ServiceContext};
use codeterm_neighbors::{
    adapter::FirebaseTransport,
    registry::RegistrySignal,
    supervisor::{supervise_documents, DocumentSession, ListenerState, ReceptionEvents},
    Error, Result,
};
use serde_json::json;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tokio::sync::watch;

#[derive(Default)]
pub(crate) struct UpdateCell {
    generation: AtomicU64,
    task: parking_lot::Mutex<Option<Handle>>,
}

struct Handle {
    task: tokio::task::JoinHandle<()>,
    stopped: watch::Sender<bool>,
    rearm: Arc<super::recovery::Rearm>,
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
        let generation = self.neighbor_updates().generation.load(Ordering::Acquire);
        let service = self.clone();
        self.start_neighbor_updates_with(
            move || {
                let service = service.clone();
                let account = account.clone();
                async move { run(service, account, generation).await }
            },
            Arc::new(codeterm_neighbors::supervisor::RuntimeClock),
        );
    }

    fn start_neighbor_updates_with<F, A>(
        &self,
        attempt: A,
        clock: Arc<dyn codeterm_neighbors::supervisor::RetryClock>,
    ) where
        F: std::future::Future<Output = Result<()>> + Send + 'static,
        A: FnMut() -> F + Send + 'static,
    {
        let cell = self.neighbor_updates();
        let mut slot = cell.task.lock();
        if slot.is_some() {
            return;
        }
        let (stop, receiver) = watch::channel(false);
        let rearm = Arc::new(super::recovery::Rearm::default());
        let task_rearm = rearm.clone();
        let task = tokio::spawn(async move {
            let _ = super::recovery::run(&task_rearm, clock.as_ref(), attempt, || {
                let mut shutdown = receiver.clone();
                async move { stopped(&mut shutdown).await }
            })
            .await;
        });
        *slot = Some(Handle {
            task,
            stopped: stop,
            rearm,
        });
    }

    pub(crate) fn rearm_neighbor_updates(&self) {
        if let Some(handle) = self.neighbor_updates().task.lock().as_ref() {
            handle.rearm.signal();
        }
        self.restore_neighbor_updates_once();
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
    progress: AtomicU64,
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
            if status == ListenerState::Receiving {
                self.progress.fetch_add(1, Ordering::AcqRel);
                if let Some(handle) = self.service.neighbor_updates().task.lock().as_ref() {
                    handle.rearm.success();
                }
            }
            self.service.neighbor_watch_state("account", status, None);
        }
    }

    async fn refresh(&self, signal: &RegistrySignal, seen: &mut notifications::Seen) -> Result<()> {
        self.current()?;
        let history_generation = self.service.neighbor_history_generation();
        if let Some(pause) = &signal.pause {
            self.service.observe_neighbor_pause(
                &self.transport.uid,
                &pause.friendship_id,
                pause.paused,
            );
            self.service
                .settle_neighbor_pause(&self.metadata, &pause.friendship_id)
                .await?;
            self.current()?;
        }
        for friendship in &signal.removed {
            self.service
                .end_neighbor_friendship(&self.metadata, friendship, history_generation)
                .await?;
            self.current()?;
        }
        if signal.receiver_revision.is_some() {
            self.service
                .neighbor_receiver_changed(signal.receiver_friendship.as_deref())
                .await
                .map_err(|_| Error::Transport)?;
            self.current()?;
        }
        let pause_versions = self.service.neighbor_cell().pauses.snapshot();
        let mut list = self
            .transport
            .calls
            .invoke("friendsList", json!({}))
            .await?;
        self.current()?;
        self.service
            .reconcile_neighbor_pauses(&self.transport.uid, &list, &pause_versions)?;
        self.service
            .end_missing_neighbor_friendships(&self.metadata, &list, history_generation)
            .await?;
        let settings = self.metadata.settings().await?;
        annotate_list(&mut list, &settings).map_err(|_| Error::Transport)?;
        self.service.project_neighbor_pauses(&mut list, &settings);
        self.service.remember_neighbor_names(&list);
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
        self.service.remember_neighbor_registry(&list);
        if list["access"] != "paid" {
            self.service.stop_neighbor_listeners();
        } else {
            self.service
                .refresh_neighbor_registry_listeners()
                .await
                .map_err(|_| Error::Transport)?;
            self.current()?;
        }
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
                service.create_in_app_notice_record(
                    crate::service::notification::InAppNotice::neighbor_connection(
                        notice.title,
                        format!(
                            "neighbor:{}:{}",
                            codeterm_neighbors::crypto::sha256_hex(uid.as_bytes()),
                            notice.event_id
                        ),
                    ),
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
) -> Result<()> {
    if service.neighbor_access_locked().await {
        return Err(Error::Locked);
    }
    let expected = account
        .current_session()
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
    let installation_id = service
        .installation_id()
        .await
        .map_err(|_| Error::Transport)?
        .as_str()
        .to_owned();
    let transport = FirebaseTransport {
        installation_id,
        project: if endpoints.qa {
            "domios-signin-qa"
        } else {
            "domios-d9357"
        }
        .into(),
        uid: uid.clone(),
        calls: Arc::new(Calls {
            own_keys: Default::default(),
            receiver_status: Default::default(),
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
        progress: AtomicU64::new(0),
    };
    supervise_documents(&session).await
}

#[async_trait::async_trait]
impl ReceptionEvents for Session {
    fn state(&self, status: ListenerState) {
        Session::state(self, status);
    }
    fn failure(&self, error: Error) {
        if self.current().is_err() {
            return;
        }
        self.service.forget_neighbor_registry();
        self.service.neighbor_watch_failure("account", error);
    }
    async fn retry_after_failure(&self, attempt: usize) -> Result<()> {
        codeterm_neighbors::supervisor::reconnect_backoff(attempt).await;
        self.current()
    }
}

#[async_trait::async_trait]
impl DocumentSession for Session {
    fn progress(&self) -> u64 {
        self.progress.load(Ordering::Acquire)
    }
    async fn stopped(&self) {
        std::future::pending::<()>().await;
    }
    async fn listen(&self) -> Result<()> {
        self.current()?;
        let mut seen = self
            .metadata
            .read("registry-notifications.json".into())
            .await?
            .unwrap_or_default();
        let mut listener = self.transport.listen_registry().await?;
        let mut revisions = delivery::Revisions::default();
        loop {
            let signal = listener.next().await?.ok_or(Error::Transport)?;
            self.current()?;
            let Some(catch_up) = revisions.observe(signal.revision) else {
                continue;
            };
            if catch_up || signal.delivery.is_none() && signal.read.is_none() && !signal.ignored {
                self.refresh(&signal, &mut seen).await?;
            }
            self.delivery(&signal, catch_up).await?;
            self.state(ListenerState::Receiving);
        }
    }
}
