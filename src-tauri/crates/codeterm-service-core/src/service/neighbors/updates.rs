mod delivery;
mod notifications;
mod ports;
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
        if slot
            .as_ref()
            .is_some_and(|handle| !handle.task.is_finished())
        {
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
    transport: Arc<FirebaseTransport>,
    ports: Arc<dyn ports::SessionPorts>,
    clock: Arc<dyn codeterm_neighbors::supervisor::RetryClock>,
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
            self.service.neighbor_watch_state("account", status, None);
        }
    }

    async fn project(
        &self,
        snapshot: codeterm_neighbors::state::Snapshot,
        _seen: &mut notifications::Seen,
    ) -> Result<()> {
        self.current()?;
        if !same_session(self.account.session().await.as_ref(), &self.expected) {
            return Err(Error::Revoked);
        }
        let previous = self.service.peek_neighbor_projection(
            &self.transport.uid,
            self.expected.login_generation,
            "friendsList",
            &self.transport.installation_id,
        );
        self.service.select_neighbor_projection(&self.transport.uid, self.expected.login_generation).await?;
        let version = snapshot.version;
        let owner = self.reconciliation_owner();
        let applied = self.service
            .consume_neighbor_projection(
                &self.transport.uid,
                self.expected.login_generation,
                snapshot,
                move |list| async move {
                    let mut seen = owner.metadata.read("registry-notifications.json".into()).await?.unwrap_or_default();
                    owner.ports.reconcile(&owner, list, previous, &mut seen).await
                },
            )
            .await?;
        if !applied && self.service.neighbor_projection_version(&self.transport.uid, self.expected.login_generation) != Some(version) {
            return Ok(());
        }
        if applied || !self.service.neighbor_projection_activated(&self.transport.uid, self.expected.login_generation, version) {
            self.require_activation(version).await?;
            let list = self.service.peek_neighbor_projection(&self.transport.uid, self.expected.login_generation, "friendsList", &self.transport.installation_id)
                .ok_or(Error::Revoked)?;
            self.ports.activate(self, &list).await?;
            self.service.remember_neighbor_projection_activation(&self.transport.uid, self.expected.login_generation, version)?;
        }
        Ok(())
    }

    fn reconciliation_owner(&self) -> Self {
        Self {
            service: self.service.clone(), account: self.account.clone(),
            expected: self.expected.clone(), generation: self.generation,
            transport: self.transport.clone(), ports: self.ports.clone(), clock: self.clock.clone(),
            metadata: self.metadata.clone(), progress: AtomicU64::new(0),
        }
    }

    async fn require_activation(&self, version: u64) -> Result<()> {
        self.current()?;
        self.service.neighbor_reconciliation().require_readable()?;
        if !same_session(self.account.session().await.as_ref(), &self.expected)
            || self.service.neighbor_projection_version(&self.transport.uid, self.expected.login_generation) != Some(version)
        {
            return Err(Error::Revoked);
        }
        let (_, list) = self.service.neighbor_activation_ticket(&self.transport.uid, self.expected.login_generation)?;
        if list["version"] != version { return Err(Error::Revoked); }
        self.current()?;
        self.service.require_neighbors_enabled()
    }

    async fn reconcile(
        &self,
        mut list: serde_json::Value,
        previous: Option<serde_json::Value>,
        seen: &mut notifications::Seen,
    ) -> Result<()> {
        self.current()?;
        if !same_session(self.account.session().await.as_ref(), &self.expected) { return Err(Error::Revoked); }
        let history_generation = self.service.neighbor_history_generation();
        let pause_versions = self.service.neighbor_cell().pauses.snapshot();
        self.service
            .reconcile_neighbor_pauses(&self.transport.uid, &list, &pause_versions)?;
        self.service
            .end_missing_neighbor_friendships(&self.metadata, &list, history_generation)
            .await?;
        let settings = self.metadata.settings().await?;
        annotate_list(&mut list, &settings).map_err(|_| Error::InvalidEnvelope)?;
        self.service.project_neighbor_pauses(&mut list, &settings);
        self.service
            .project_neighbor_send_counts(&self.metadata, &mut list)
            .await?;
        let accepted = accepted_connections(previous.as_ref(), &list);
        let notices = notifications::changes(seen, &list, &accepted)?;
        self.ports.persist_notifications(self, seen).await?;
        self.current()?;
        self.service.remember_neighbor_projection_registry(
            &self.transport.uid,
            self.expected.login_generation,
            &list,
        )?;
        self.current()?;
        if !same_session(self.account.session().await.as_ref(), &self.expected) {
            return Err(Error::Revoked);
        }
        self.service.publish_neighbor_projection_list(
            &self.transport.uid,
            self.expected.login_generation,
            list,
        );
        for notice in notices {
            self.service.create_in_app_notice_record(
                crate::service::notification::InAppNotice::neighbor_connection(
                    notice.title,
                    format!(
                        "neighbor:{}:{}",
                        codeterm_neighbors::crypto::sha256_hex(self.transport.uid.as_bytes()),
                        notice.event_id
                    ),
                ),
            );
        }
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
    let transport = Arc::new(FirebaseTransport {
        installation_id,
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
    });
    service.select_neighbor_projection(&expected.identity.uid, expected.login_generation).await?;
    let session = Session {
        ports: Arc::new(ports::LivePorts(transport.clone())),
        clock: Arc::new(codeterm_neighbors::supervisor::RuntimeClock),
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
        self.service.neighbor_projection_unavailable();
        self.service.neighbor_watch_failure("account", error);
    }
    async fn retry_after_failure(&self, attempt: usize) -> Result<()> {
        codeterm_neighbors::supervisor::reconnect_backoff_with(self.clock.as_ref(), attempt).await;
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
        let mut listener = match self.ports.listen_state().await {
            Ok(listener) => listener,
            Err(error) => {
                self.fallback().await;
                return Err(error);
            }
        };
        let mut signals: Option<Box<dyn codeterm_neighbors::registry::RegistryListener>> = None;
        let mut revisions = delivery::Revisions::default();
        let mut health = codeterm_neighbors::supervisor::StreamHealth::default();
        let mut received_snapshot = false;
        loop {
            enum Event {
                State(Option<codeterm_neighbors::state::StateEvent>),
                Signal(Option<RegistrySignal>),
            }
            let next = tokio::select! {
                state = listener.next() => state.map(Event::State),
                signal = async {
                    match signals.as_mut() {
                        Some(listener) => listener.next().await,
                        None => std::future::pending().await,
                    }
                } => signal.map(Event::Signal),
            };
            self.current()?;
            let mut incremental = false;
            let mut observe_health = false;
            match next {
                Ok(Event::State(Some(codeterm_neighbors::state::StateEvent::Snapshot(
                    snapshot,
                )))) => {
                    incremental = received_snapshot
                        && self
                            .service
                            .neighbor_projection_version(
                                &self.transport.uid,
                                self.expected.login_generation,
                            )
                            .is_some_and(|version| snapshot.version > version);
                    received_snapshot = true;
                    observe_health = true;
                    self.project(snapshot, &mut seen).await?;
                    let list = self.service.peek_neighbor_projection(
                        &self.transport.uid,
                        self.expected.login_generation,
                        "friendsList",
                        &self.transport.installation_id,
                    );
                    let receiving = list.as_ref().is_some_and(|list| {
                        receives_friends(list, &self.transport.installation_id)
                    });
                    if receiving && signals.is_none() {
                        signals = Some(self.ports.listen_signals().await?);
                    }
                    if !receiving {
                        signals = None;
                        revisions = delivery::Revisions::default();
                    }
                }
                Ok(Event::State(Some(codeterm_neighbors::state::StateEvent::Missing))) => {
                    self.service.neighbor_projection_missing().await?;
                    self.ports.stop_receiving(self);
                    signals = None;
                    health = codeterm_neighbors::supervisor::StreamHealth::default();
                    received_snapshot = false;
                    self.fallback().await;
                }
                Ok(Event::Signal(Some(signal))) => {
                    if let Some(catch_up) = revisions.observe(signal.revision) {
                        incremental =
                            !catch_up && (signal.delivery.is_some() || signal.read.is_some());
                        observe_health = received_snapshot;
                        self.delivery(&signal, catch_up).await?;
                    }
                }
                Ok(Event::State(None) | Event::Signal(None)) => {
                    self.fallback().await;
                    return Err(Error::Transport);
                }
                Err(error) => {
                    self.fallback().await;
                    return Err(error);
                }
            }
            if observe_health && health.observe(self.clock.now(), incremental) {
                self.progress.fetch_add(1, Ordering::AcqRel);
                if let Some(handle) = self.service.neighbor_updates().task.lock().as_ref() {
                    handle.rearm.success();
                }
            }
            self.state(ListenerState::Receiving);
        }
    }
}

pub(in crate::service::neighbors) fn receives_friends(
    list: &serde_json::Value,
    installation: &str,
) -> bool {
    list["access"] == "paid"
        && list["friends"].as_array().is_some_and(|friends| {
            friends.iter().any(|friend| {
                friend["my_receiver"]["receiving"] == true
                    && friend["my_receiver"]["installation_id"] == installation
            })
        })
}

impl Session {
    async fn fallback(&self) {
        self.service.neighbor_projection_unavailable();
        if self
            .service
            .peek_neighbor_projection(
                &self.transport.uid,
                self.expected.login_generation,
                "friendsList",
                &self.transport.installation_id,
            )
            .is_some()
        {
            return;
        }
        if self.current().is_ok() && self.service.begin_neighbor_fallback() {
            let Ok(epoch) = self.service.neighbor_projection_read_epoch(
                &self.transport.uid,
                self.expected.login_generation,
            ) else {
                return;
            };
            match self.transport.calls.invoke("friendsList", json!({})).await {
                Ok(mut list) => {
                    let publish = async {
                        let settings = self.metadata.settings().await?;
                        annotate_list(&mut list, &settings).map_err(|_| Error::InvalidEnvelope)?;
                        self.service
                            .project_neighbor_send_counts(&self.metadata, &mut list)
                            .await?;
                        self.current()?;
                        if !same_session(self.account.session().await.as_ref(), &self.expected) {
                            return Err(Error::Revoked);
                        }
                        self.service.publish_neighbor_fallback(
                            &self.transport.uid,
                            self.expected.login_generation,
                            &epoch,
                            list,
                        );
                        Ok::<(), Error>(())
                    }
                    .await;
                    if let Err(error) = publish {
                        log::debug!("[neighbors] fallback display unavailable: {error}");
                    }
                }
                Err(error) => {
                    log::debug!("[neighbors] initial projection fallback unavailable: {error}")
                }
            }
        }
    }
}

fn accepted_connections(
    previous: Option<&serde_json::Value>,
    list: &serde_json::Value,
) -> Vec<notifications::Acceptance> {
    let Some(previous) = previous else {
        return vec![];
    };
    list["friends"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|friend| {
            !previous["friends"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|old| old["friendship_id"] == friend["friendship_id"])
                && friend["email"].as_str().is_some_and(|email| {
                    previous["outgoing"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|invite| {
                            invite["typed_email"]
                                .as_str()
                                .is_some_and(|invited| invited.eq_ignore_ascii_case(email))
                        })
                })
        })
        .filter_map(|friend| {
            Some(notifications::Acceptance {
                friendship_id: friend["friendship_id"].as_str()?.into(),
                event_id: format!(
                    "projection:{}:{}",
                    friend["friendship_id"].as_str()?,
                    friend["since"].as_u64()?
                ),
            })
        })
        .collect()
}
