use super::{
    history::{
        handling::{Owner, State},
        History, HistoryCell,
    },
    storage::Metadata,
    ServiceContext,
};
use codeterm_neighbors::{Error, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Cell {
    summaries: parking_lot::Mutex<BTreeMap<String, (u64, Value)>>,
    edges: parking_lot::Mutex<BTreeMap<String, PtyEdge>>,
    pending: parking_lot::Mutex<BTreeMap<String, PtyEdge>>,
    worker: std::sync::atomic::AtomicBool,
}
impl Cell {
    pub(super) fn clear(&self) {
        self.summaries.lock().clear();
        self.edges.lock().clear();
        self.pending.lock().clear();
    }
}
#[derive(Clone)]
struct PtyEdge {
    owner: Owner,
    version: u64,
    started: u64,
    ended: u64,
    working: bool,
    error: bool,
    generation: u64,
}

pub(super) fn incoming(
    message: &codeterm_neighbors::wire::InboxMessage,
    body: &str,
    reason: Option<&str>,
) -> super::history::Entry {
    let mut handling = super::history::handling::Handling::default();
    handling.set(State::Waiting, reason, now());
    super::history::Entry {
        received_by: None,
        flags: Vec::new(),
        flag_callers: Vec::new(),
        handling,
        direction: "in".into(),
        author: super::history::Author::Person("friend".into()),
        sender_uid: message.doc.envelope.sender_uid.clone(),
        body: body.into(),
        at: message.doc.received_at.seconds * 1000
            + u64::from(message.doc.received_at.nanoseconds / 1_000_000),
        message_id: message.doc.envelope.message_id.clone(),
        kind: message.doc.envelope.kind,
        in_reply_to: message.doc.envelope.in_reply_to.clone(),
        outcome: None,
        reason: None,
        limit: None,
        resets_at: None,
        retry_after_seconds: None,
        delivered_at: None,
        read_at: None,
        delivery_status: None,
        submission: None,
    }
}

pub(super) fn summary(history: &History) -> Value {
    json!(history.entries.iter().filter(|entry| entry.direction=="in").filter_map(|entry| entry.handling.handling_state.map(|state| json!({"message_id":entry.message_id,"handling_state":state,"handling_reason":entry.handling.handling_reason,"handling_updated_at":entry.handling.handling_updated_at}))).collect::<Vec<_>>())
}

pub(super) fn now() -> u64 {
    chrono::Utc::now().timestamp_millis().max(0) as u64
}

impl ServiceContext {
    pub(super) fn remember_neighbor_handling(&self, friend: &str, summary: Value, generation: u64) {
        let mut summaries = self.neighbor_cell().handling.summaries.lock();
        if summaries.len() >= 200 && !summaries.contains_key(friend) {
            if let Some(key) = summaries.keys().next().cloned() {
                summaries.remove(&key);
            }
        }
        summaries.insert(friend.into(), (generation, summary));
    }
    pub(super) fn neighbor_handling_status(&self) -> Value {
        let generation = self.neighbor_history_generation();
        let summaries: serde_json::Map<String, Value> = self
            .neighbor_cell()
            .handling
            .summaries
            .lock()
            .iter()
            .filter(|(_, (recorded, _))| *recorded == generation)
            .map(|(friend, (_, value))| (friend.clone(), value.clone()))
            .collect();
        Value::Object(summaries)
    }

    pub(super) async fn mutate_neighbor_handling(
        &self,
        metadata: &Metadata,
        friend: &str,
        generation: u64,
        change: impl FnOnce(&mut History) -> bool + Send,
    ) -> Result<()> {
        let _guard = self
            .extensions
            .get_or_init(HistoryCell::default)
            .0
            .lock()
            .await;
        self.require_neighbor_history_generation(generation)?;
        let mut history = metadata.history(friend).await?;
        if history.ended || !change(&mut history) {
            return Ok(());
        }
        history.reconcile_outcomes(now());
        let history = tokio::task::spawn_blocking(move || {
            history.bound()?;
            Ok::<_, Error>(history)
        })
        .await
        .map_err(|_| Error::CursorStore)??;
        let snapshot = summary(&history);
        let read_through = history.concierge_read_through();
        self.require_neighbor_history_generation(generation)?;
        metadata
            .write(format!("history-{friend}.json"), history)
            .await?;
        self.remember_neighbor_handling(friend, snapshot, generation);
        self.ui
            .emit("neighbor-history-changed", json!({"friendship_id":friend}));
        drop(_guard);
        if let Some(through) = read_through {
            self.queue_neighbor_read(friend, through, generation);
        }
        Ok(())
    }

    pub(super) fn neighbor_handling_owner(&self, tab: &str) -> Option<Owner> {
        let mux = self.mux.read();
        let target = mux.get_tab(tab)?;
        let mut owner = Owner {
            tab: tab.into(),
            session: target.agent_session_id.clone()?,
            nonce: target.session_binding_nonce.clone()?,
            runtime: target.is_chat_backend(),
            turn: None,
            edge: 0,
        };
        drop(mux);
        if let Some(edge) = self
            .neighbor_cell()
            .handling
            .edges
            .lock()
            .get(tab)
            .filter(|edge| edge.owner.session == owner.session && edge.owner.nonce == owner.nonce)
        {
            owner.edge = if edge.working {
                edge.started.saturating_sub(1)
            } else {
                edge.version
            };
        }
        Some(owner)
    }

    pub(super) async fn prepare_neighbor_handling(
        &self,
        metadata: &Metadata,
        friend: &str,
        message: &str,
        tab: &str,
        generation: u64,
    ) -> Result<()> {
        let owner = self.neighbor_handling_owner(tab).ok_or(Error::Gateway)?;
        self.mutate_neighbor_handling(metadata, friend, generation, |history| {
            let Some(entry) = history
                .entries
                .iter_mut()
                .find(|entry| entry.direction == "in" && entry.message_id == message)
            else {
                return false;
            };
            if matches!(
                entry.handling.handling_state,
                Some(State::Answered | State::Refused | State::Stopped)
            ) {
                return false;
            }
            entry.handling.handling_owner = Some(owner);
            entry
                .handling
                .set(State::Waiting, Some("queued_for_concierge"), now());
            true
        })
        .await
    }

    pub(super) async fn neighbor_handling_delivery(
        &self,
        metadata: &Metadata,
        friend: &str,
        message: &str,
        state: State,
        reason: Option<&str>,
        generation: u64,
    ) -> Result<()> {
        let mut tab = None;
        self.mutate_neighbor_handling(metadata, friend, generation, |history| {
            let Some(entry) = history
                .entries
                .iter_mut()
                .find(|entry| entry.direction == "in" && entry.message_id == message)
            else {
                return false;
            };
            if state == State::NoReply && entry.handling.handling_owner.is_some() {
                return false;
            }
            if matches!(
                entry.handling.handling_state,
                Some(
                    State::Answered
                        | State::Refused
                        | State::NoReply
                        | State::Working
                        | State::Stopped
                )
            ) {
                return false;
            }
            tab = entry
                .handling
                .handling_owner
                .as_ref()
                .map(|owner| owner.tab.clone());
            entry.handling.set(state, reason, now())
        })
        .await?;
        if state == State::DeliveredToConcierge {
            let edge = tab.and_then(|tab| {
                self.neighbor_cell()
                    .handling
                    .edges
                    .lock()
                    .get(&tab)
                    .cloned()
            });
            if let Some(edge) = edge {
                self.apply_neighbor_pty_edge(&edge).await?;
            }
        }
        Ok(())
    }

    pub(crate) fn observe_neighbor_pty_handling(
        &self,
        tab: &str,
        status: crate::agent::status::TabStatus,
    ) {
        use crate::agent::status::TabStatus;
        if !matches!(
            status,
            TabStatus::Working | TabStatus::Idle | TabStatus::Error
        ) {
            return;
        }
        if self.require_neighbors_enabled().is_err() {
            return;
        }
        let Some(owner) = self
            .neighbor_handling_owner(tab)
            .filter(|owner| !owner.runtime)
        else {
            return;
        };
        if !self
            .mux
            .read()
            .get_tab(tab)
            .and_then(|tab| tab.neighbor_authority.as_ref())
            .is_some_and(|authority| authority.root_tab_id.is_none())
        {
            return;
        }
        let cell = &self.neighbor_cell().handling;
        let edge = {
            let mut edges = cell.edges.lock();
            if edges.len() >= 200 && !edges.contains_key(tab) {
                if let Some(key) = edges.keys().next().cloned() {
                    edges.remove(&key);
                }
            }
            let edge = edges.entry(tab.into()).or_insert(PtyEdge {
                owner: owner.clone(),
                version: 0,
                started: 0,
                ended: 0,
                working: false,
                error: false,
                generation: self.neighbor_history_generation(),
            });
            if edge.owner.nonce != owner.nonce || edge.owner.session != owner.session {
                *edge = PtyEdge {
                    owner,
                    version: 0,
                    started: 0,
                    ended: 0,
                    working: false,
                    error: false,
                    generation: self.neighbor_history_generation(),
                };
            }
            edge.version += 1;
            edge.working = status == TabStatus::Working;
            edge.error = status == TabStatus::Error;
            if edge.working {
                edge.started = edge.version;
            } else {
                edge.ended = edge.version;
            }
            edge.clone()
        };
        {
            let mut pending = cell.pending.lock();
            if pending.len() >= 200 && !pending.contains_key(tab) {
                if let Some(key) = pending.keys().next().cloned() {
                    pending.remove(&key);
                }
            }
            pending.insert(tab.into(), edge);
        }
        if !cell.worker.swap(true, std::sync::atomic::Ordering::AcqRel) {
            let service = self.clone();
            crate::async_runtime::spawn(async move {
                loop {
                    let pending =
                        std::mem::take(&mut *service.neighbor_cell().handling.pending.lock());
                    for edge in pending.values() {
                        if let Err(error) = service.apply_neighbor_pty_edge(edge).await {
                            log::debug!("[neighbors] PTY incoming handling: {error}");
                        }
                    }
                    let pending = service.neighbor_cell().handling.pending.lock();
                    if pending.is_empty() {
                        service
                            .neighbor_cell()
                            .handling
                            .worker
                            .store(false, std::sync::atomic::Ordering::Release);
                        return;
                    }
                }
            });
        }
    }

    async fn apply_neighbor_pty_edge(&self, edge: &PtyEdge) -> Result<()> {
        if self
            .require_neighbor_history_generation(edge.generation)
            .is_err()
        {
            return Ok(());
        }
        let authority = self
            .mux
            .read()
            .get_tab(&edge.owner.tab)
            .and_then(|tab| tab.neighbor_authority.clone());
        let Some(authority) = authority else {
            return Ok(());
        };
        if self
            .neighbor_handling_owner(&edge.owner.tab)
            .is_none_or(|owner| {
                owner.nonce != edge.owner.nonce || owner.session != edge.owner.session
            })
        {
            return Ok(());
        }
        let account = self
            .ext::<crate::service::account::AccountCell>()
            .0
            .get()
            .cloned();
        let Some(account) = account else {
            return Ok(());
        };
        if account
            .session()
            .await
            .as_ref()
            .is_none_or(|session| session.identity.uid != authority.uid)
        {
            return Ok(());
        }
        let metadata = Metadata::new(
            self.account_data_root().map_err(|_| Error::CursorStore)?,
            &authority.uid,
        );
        if metadata
            .settings()
            .await?
            .bindings
            .get(&authority.friendship_id)
            != Some(&edge.owner.tab)
        {
            return Ok(());
        }
        self.mutate_neighbor_handling(
            &metadata,
            &authority.friendship_id,
            edge.generation,
            |history| {
                let mut changed = false;
                for entry in history
                    .entries
                    .iter_mut()
                    .filter(|entry| entry.direction == "in")
                {
                    let Some(owner) = entry.handling.handling_owner.as_ref() else {
                        continue;
                    };
                    if owner.runtime
                        || owner.tab != edge.owner.tab
                        || owner.session != edge.owner.session
                        || owner.nonce != edge.owner.nonce
                        || !matches!(
                            entry.handling.handling_state,
                            Some(State::DeliveredToConcierge | State::Working)
                        )
                    {
                        continue;
                    }
                    if edge.working {
                        changed |= entry.handling.set(State::Working, None, now());
                    } else if edge.started > owner.edge && edge.ended > edge.started {
                        changed |= entry.handling.set(
                            if edge.error {
                                State::Failed
                            } else {
                                State::NoReply
                            },
                            edge.error.then_some("concierge_turn_failed"),
                            now(),
                        );
                    }
                }
                changed
            },
        )
        .await
    }

    pub(crate) async fn neighbor_runtime_handling_event(
        &self,
        tab: &str,
        nonce: &str,
        thread: &str,
        turn: &str,
        message: Option<&str>,
        state: State,
    ) -> Result<()> {
        let generation = self.neighbor_history_generation();
        let (authority, owner) = {
            let mux = self.mux.read();
            let target = mux.get_tab(tab).ok_or(Error::Gateway)?;
            if target.session_binding_nonce.as_deref() != Some(nonce)
                || target.agent_session_id.as_deref() != Some(thread)
            {
                return Ok(());
            }
            let Some(authority) = target
                .neighbor_authority
                .as_ref()
                .filter(|authority| authority.root_tab_id.is_none())
                .cloned()
            else {
                return Ok(());
            };
            (
                authority,
                Owner {
                    tab: tab.into(),
                    session: thread.into(),
                    nonce: nonce.into(),
                    runtime: true,
                    turn: Some(turn.into()),
                    edge: 0,
                },
            )
        };
        let metadata = Metadata::new(
            self.account_data_root().map_err(|_| Error::CursorStore)?,
            &authority.uid,
        );
        let account = self
            .ext::<crate::service::account::AccountCell>()
            .0
            .get()
            .cloned();
        let Some(account) = account else {
            return Ok(());
        };
        if account
            .session()
            .await
            .as_ref()
            .is_none_or(|session| session.identity.uid != authority.uid)
        {
            return Ok(());
        }
        if metadata
            .settings()
            .await?
            .bindings
            .get(&authority.friendship_id)
            .map(String::as_str)
            != Some(tab)
        {
            return Ok(());
        }
        self.mutate_neighbor_handling(&metadata, &authority.friendship_id, generation, |history| {
            let mut changed = false;
            for entry in history
                .entries
                .iter_mut()
                .filter(|entry| entry.direction == "in")
            {
                let Some(previous) = entry.handling.handling_owner.as_ref() else {
                    continue;
                };
                if previous.tab != tab
                    || previous.nonce != nonce
                    || previous.session != thread
                    || !previous.runtime
                {
                    continue;
                }
                let evidence = message == Some(entry.message_id.as_str());
                if entry.handling.handling_state == Some(State::Failed) && !evidence {
                    continue;
                }
                if !evidence && previous.turn.as_deref() != Some(turn) {
                    continue;
                }
                if matches!(
                    entry.handling.handling_state,
                    Some(State::Answered | State::Refused | State::Stopped | State::NoReply)
                ) {
                    continue;
                }
                if evidence {
                    entry.handling.handling_owner = Some(owner.clone());
                    changed = true;
                }
                changed |= entry.handling.set(
                    state,
                    (state == State::Failed).then_some("concierge_turn_failed"),
                    now(),
                );
            }
            changed
        })
        .await
    }

    pub(super) async fn stop_neighbor_concierge(
        &self,
        friend: &str,
    ) -> std::result::Result<Value, String> {
        self.stop_neighbor_concierge_with(friend, |tab| {
            crate::service::stop_execution::stop_agent_turn(self.clone(), tab)
        })
        .await
    }
    async fn stop_neighbor_concierge_with<F, Fut>(
        &self,
        friend: &str,
        stop: F,
    ) -> std::result::Result<Value, String>
    where
        F: FnOnce(String) -> Fut + Send,
        Fut: std::future::Future<Output = std::result::Result<Value, String>> + Send,
    {
        self.require_neighbors_enabled()
            .map_err(|error| error.to_string())?;
        let generation = self.neighbor_history_generation();
        if !codeterm_neighbors::wire::hex_id(friend, 64) {
            return Err(Error::InvalidRequest.to_string());
        }
        let account = self
            .ext::<crate::service::account::AccountCell>()
            .0
            .get()
            .cloned()
            .ok_or("Neighbor account unavailable")?;
        let session = account
            .session()
            .await
            .ok_or("Neighbor account is signed out")?;
        let metadata = Metadata::new(self.account_data_root()?, &session.identity.uid);
        let tab = metadata
            .settings()
            .await
            .map_err(|error| error.to_string())?
            .bindings
            .get(friend)
            .cloned()
            .ok_or("neighbor_forbidden: No concierge is bound")?;
        let owner = self
            .neighbor_handling_owner(&tab)
            .ok_or("neighbor_forbidden: Concierge is unavailable")?;
        let turn = crate::service::agent_runtime::engine::for_tab(&tab)
            .and_then(|runtime| runtime.context.lock().active_turn_id.clone());
        let history = metadata
            .history(friend)
            .await
            .map_err(|error| error.to_string())?;
        let mut ids: Vec<_> = history
            .entries
            .iter()
            .filter(|entry| {
                entry.direction == "in"
                    && entry.handling.active()
                    && entry
                        .handling
                        .handling_owner
                        .as_ref()
                        .is_some_and(|current| {
                            current.tab == owner.tab
                                && current.session == owner.session
                                && current.nonce == owner.nonce
                                && if owner.runtime {
                                    turn.is_some() && current.turn == turn
                                } else {
                                    entry.handling.handling_state == Some(State::Working)
                                }
                        })
            })
            .map(|entry| entry.message_id.clone())
            .collect();
        if metadata
            .settings()
            .await
            .map_err(|error| error.to_string())?
            .bindings
            .get(friend)
            != Some(&tab)
            || self.neighbor_handling_owner(&tab).is_none_or(|current| {
                current.nonce != owner.nonce || current.session != owner.session
            })
        {
            return Err("neighbor_forbidden: Concierge binding changed".into());
        }
        self.require_neighbor_history_generation(generation)
            .map_err(|error| error.to_string())?;
        if account
            .session()
            .await
            .as_ref()
            .is_none_or(|current| current.identity.uid != session.identity.uid)
        {
            return Err("neighbor_forbidden: Neighbor account changed".into());
        }
        let result = stop(tab.clone()).await?;
        let stopped = result["stopped"].as_bool().unwrap_or(false);
        if stopped {
            self.mutate_neighbor_handling(&metadata, friend, generation, |history| {
                let mut changed = false;
                for entry in history
                    .entries
                    .iter_mut()
                    .filter(|entry| entry.direction == "in" && ids.contains(&entry.message_id))
                {
                    if !matches!(
                        entry.handling.handling_state,
                        Some(State::Answered | State::Refused)
                    ) {
                        changed |= entry
                            .handling
                            .set(State::Stopped, Some("owner_stopped"), now());
                    }
                }
                changed
            })
            .await
            .map_err(|error| error.to_string())?;
        } else {
            ids.clear();
        }
        Ok(
            json!({"friendship_id":friend,"gateway_tab":tab,"stopped":result["stopped"].as_bool().unwrap_or(false),"message_ids":ids}),
        )
    }

    pub(super) async fn reconcile_neighbor_handling_restore(
        &self,
        metadata: &Metadata,
        friend: &str,
        generation: u64,
    ) -> Result<()> {
        if self.require_neighbors_enabled().is_err() {
            return Ok(());
        }
        let settings = metadata.settings().await?;
        let current = settings
            .bindings
            .get(friend)
            .and_then(|tab| self.neighbor_handling_owner(tab));
        let paused = self.neighbor_effective_pause(&settings, friend);
        self.mutate_neighbor_handling(metadata, friend, generation, |history| {
            let mut changed = false;
            for entry in history
                .entries
                .iter_mut()
                .filter(|entry| entry.direction == "in" && entry.handling.active())
            {
                if let Some(owner) = entry.handling.handling_owner.as_ref() {
                    if current.as_ref().is_none_or(|current| {
                        owner.tab != current.tab
                            || owner.nonce != current.nonce
                            || owner.session != current.session
                    }) {
                        changed |=
                            entry
                                .handling
                                .set(State::Failed, Some("concierge_restarted"), now());
                    }
                } else if paused || current.is_none() {
                    changed |= entry.handling.set(
                        State::Waiting,
                        Some(if paused {
                            "paused"
                        } else {
                            "concierge_missing"
                        }),
                        now(),
                    );
                }
            }
            changed
        })
        .await
    }
}

#[cfg(test)]
mod tests;
