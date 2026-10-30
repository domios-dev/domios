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
        if slot.as_ref().is_some_and(|handle| !handle.task.is_finished()) {
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
            self.service.neighbor_watch_state("account", status, None);
        }
    }

    async fn project(&self, snapshot: codeterm_neighbors::state::Snapshot, seen: &mut notifications::Seen) -> Result<()> {
        self.current()?;
        if !same_session(self.account.session().await.as_ref(), &self.expected) { return Err(Error::Revoked); }
        let previous = self.service.peek_neighbor_projection(&self.transport.uid, self.expected.login_generation, "friendsList", &self.transport.installation_id);
        self.service.consume_neighbor_projection(&self.transport.uid, self.expected.login_generation, snapshot, |mut list| async move {
        let history_generation = self.service.neighbor_history_generation();
        let pause_versions = self.service.neighbor_cell().pauses.snapshot();
        self.service.reconcile_neighbor_pauses(&self.transport.uid, &list, &pause_versions)?;
        self.service.end_missing_neighbor_friendships(&self.metadata, &list, history_generation).await?;
        let settings = self.metadata.settings().await?;
        annotate_list(&mut list, &settings).map_err(|_| Error::InvalidEnvelope)?;
        self.service.project_neighbor_pauses(&mut list, &settings);
        self.service.project_neighbor_send_counts(&self.metadata, &mut list).await?;
        self.service.remember_neighbor_names(&list);
        let accepted = accepted_connections(previous.as_ref(), &list);
        let notices = notifications::changes(seen, &list, &accepted)?;
        self.metadata.write("registry-notifications.json".into(), serde_json::to_value(&*seen).map_err(|_| Error::CursorStore)?).await?;
        self.current()?;
        self.service.remember_neighbor_registry(&list);
        if list["access"] != "paid" || self.service.neighbor_access_locked().await {
            self.service.pause_neighbor_reception();
        } else {
            self.service.refresh_neighbor_registry_listeners().await.map_err(|_| Error::Transport)?;
        }
        self.current()?;
        if !same_session(self.account.session().await.as_ref(), &self.expected) { return Err(Error::Revoked); }
        self.service.ui.emit("neighbors-changed", json!({"uid":self.transport.uid,"list":list}));
        for notice in notices {
            self.service.create_in_app_notice_record(crate::service::notification::InAppNotice::neighbor_connection(
                notice.title, format!("neighbor:{}:{}", codeterm_neighbors::crypto::sha256_hex(self.transport.uid.as_bytes()), notice.event_id)));
        }
        Ok(())
        }).await?;
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
            service: service.clone(),
            uid,
            generation: expected.login_generation,
        }),
    };
    service.select_neighbor_projection(&expected.identity.uid, expected.login_generation);
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
        self.service.neighbor_projection_unavailable();
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
        let mut listener = match self.transport.listen_state().await {
            Ok(listener) => listener,
            Err(error) => { self.fallback().await; return Err(error); }
        };
        let mut signals: Option<Box<dyn codeterm_neighbors::registry::RegistryListener>> = None;
        let mut revisions = delivery::Revisions::default();
        loop {
            enum Event { State(Option<codeterm_neighbors::state::StateEvent>), Signal(Option<RegistrySignal>) }
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
            match next {
                Ok(Event::State(Some(codeterm_neighbors::state::StateEvent::Snapshot(snapshot)))) => {
                    self.project(snapshot, &mut seen).await?;
                    let list = self.service.peek_neighbor_projection(&self.transport.uid, self.expected.login_generation, "friendsList", &self.transport.installation_id);
                    let receiving = list.as_ref().is_some_and(|list| receives_friends(list, &self.transport.installation_id));
                    if receiving && signals.is_none() { signals = Some(self.transport.listen_registry().await?); }
                    if !receiving { signals = None; revisions = delivery::Revisions::default(); }
                }
                Ok(Event::State(Some(codeterm_neighbors::state::StateEvent::Missing))) => {
                    self.service.neighbor_projection_missing();
                    self.service.pause_neighbor_reception();
                    signals = None;
                    self.fallback().await;
                }
                Ok(Event::Signal(Some(signal))) => {
                    if let Some(catch_up) = revisions.observe(signal.revision) { self.delivery(&signal, catch_up).await?; }
                }
                Ok(Event::State(None) | Event::Signal(None)) => { self.fallback().await; return Err(Error::Transport); }
                Err(error) => { self.fallback().await; return Err(error); }
            }
            self.progress.fetch_add(1, Ordering::AcqRel);
            if let Some(handle) = self.service.neighbor_updates().task.lock().as_ref() { handle.rearm.success(); }
            self.state(ListenerState::Receiving);
        }
    }
}


pub(in crate::service::neighbors) fn receives_friends(list: &serde_json::Value, installation: &str) -> bool {
    list["access"] == "paid" && list["friends"].as_array().is_some_and(|friends| {
        friends.iter().any(|friend| friend["my_receiver"]["receiving"] == true
            && friend["my_receiver"]["installation_id"] == installation)
    })
}


impl Session {
    async fn fallback(&self) {
        self.service.neighbor_projection_unavailable();
        if self.current().is_ok() && self.service.begin_neighbor_fallback() {
            if let Err(error) = self.service.neighbor_registry_owned(super::NeighborCommand::List {}).await {
                log::debug!("[neighbors] initial projection fallback unavailable: {error}");
            }
        }
    }
}

fn accepted_connections(previous: Option<&serde_json::Value>, list: &serde_json::Value) -> Vec<notifications::Acceptance> {
    let Some(previous) = previous else { return vec![]; };
    list["friends"].as_array().into_iter().flatten().filter(|friend| {
        !previous["friends"].as_array().into_iter().flatten().any(|old| old["friendship_id"] == friend["friendship_id"])
            && friend["email"].as_str().is_some_and(|email| previous["outgoing"].as_array().into_iter().flatten().any(|invite| invite["typed_email"].as_str().is_some_and(|invited| invited.eq_ignore_ascii_case(email))))
    }).filter_map(|friend| Some(notifications::Acceptance { friendship_id: friend["friendship_id"].as_str()?.into(),
        event_id: format!("projection:{}:{}", friend["friendship_id"].as_str()?, friend["since"].as_u64()?) })).collect()
}
