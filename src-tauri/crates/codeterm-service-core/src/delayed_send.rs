use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use crate::persisted_state::{Loaded, StateFile};
use crate::service::ServiceState;

mod delivery;
pub mod recurrence;
mod system_entries;
use recurrence::RecurrenceRule;
pub use system_entries::{
    list_views as system_heartbeat_views, system_entry_refusal, DeliveryTickView, MessageOrigin,
    SystemEntryRefusal,
};

mod types;
pub use types::{DelayedMessage, DelayedMessageState, DelayedMessageView, ReminderCreator};

const QUEUE_VERSION: u32 = 3;

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QueueFile {
    version: u32,
    messages: Vec<DelayedMessage>,
}

#[derive(Default)]
struct QueueInner {
    messages: Vec<DelayedMessage>,
}

#[derive(Clone)]
pub struct DelayedSendQueue {
    path: PathBuf,
    inner: Arc<Mutex<QueueInner>>,
    #[cfg(test)]
    persistence_refusal: Arc<Mutex<Option<String>>>,
}

impl DelayedSendQueue {
    pub(crate) fn empty(path: PathBuf) -> Self {
        Self {
            path,
            inner: Arc::new(Mutex::new(QueueInner::default())),
            #[cfg(test)]
            persistence_refusal: Default::default(),
        }
    }

    pub fn open(path: PathBuf) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("create {}: {}", parent.display(), e))?;
        }
        let mut messages = match queue_file(&path).load(|text| parse_queue(&path, text)) {
            Loaded::Parsed(messages)
            | Loaded::Recovered {
                value: messages, ..
            } => messages,
            Loaded::Missing | Loaded::Unrecoverable { .. } => Vec::new(),
        };
        for msg in &mut messages {
            msg.last_tick_send_sequence = 0;
            let start = msg.series_start_at_ms.unwrap_or(msg.scheduled_at_ms);
            let invalid_rule = msg
                .recurrence
                .as_ref()
                .and_then(|rule| recurrence::validate(rule, start).err());
            if let Some(error) = invalid_rule {
                msg.recurrence = None;
                msg.state = DelayedMessageState::Failed;
                msg.error = Some(error);
                continue;
            }
            if msg.state == DelayedMessageState::Sending {
                msg.state = DelayedMessageState::Queued;
                if msg.recurrence.is_some() {
                    msg.last_tick_unconfirmed = true;
                }
            }
        }
        sort_messages(&mut messages);
        Ok(Self {
            path,
            inner: Arc::new(Mutex::new(QueueInner { messages })),
            #[cfg(test)]
            persistence_refusal: Default::default(),
        })
    }

    pub fn list(&self, tab_id: Option<&str>) -> Vec<DelayedMessage> {
        let mut messages: Vec<_> = self
            .inner
            .lock()
            .messages
            .iter()
            .filter(|msg| tab_id.map_or(true, |tab_id| msg.tab_id == tab_id))
            .cloned()
            .collect();
        sort_messages(&mut messages);
        messages
    }

    #[allow(clippy::too_many_arguments)]
    pub fn enqueue(
        &self,
        tab_id: String,
        workspace_id: Option<String>,
        session_id_at_create: Option<String>,
        text: String,
        scheduled_at_ms: Option<u64>,
        creator: ReminderCreator,
    ) -> Result<DelayedMessage, String> {
        self.enqueue_with_recurrence(
            tab_id,
            workspace_id,
            session_id_at_create,
            text,
            scheduled_at_ms,
            creator,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn enqueue_with_recurrence(
        &self,
        tab_id: String,
        workspace_id: Option<String>,
        session_id_at_create: Option<String>,
        text: String,
        scheduled_at_ms: Option<u64>,
        creator: ReminderCreator,
        recurrence: Option<RecurrenceRule>,
        team_id: Option<String>,
    ) -> Result<DelayedMessage, String> {
        self.enqueue_with_recurrence_at_sequence(
            tab_id,
            workspace_id,
            session_id_at_create,
            text,
            scheduled_at_ms,
            creator,
            recurrence,
            team_id,
            0,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn enqueue_with_recurrence_at_sequence(
        &self,
        tab_id: String,
        workspace_id: Option<String>,
        session_id_at_create: Option<String>,
        text: String,
        scheduled_at_ms: Option<u64>,
        creator: ReminderCreator,
        recurrence: Option<RecurrenceRule>,
        team_id: Option<String>,
        last_tick_send_sequence: u64,
    ) -> Result<DelayedMessage, String> {
        let msg = Self::prepare_message(
            tab_id,
            workspace_id,
            session_id_at_create,
            text,
            scheduled_at_ms,
            creator,
            recurrence,
            team_id,
            last_tick_send_sequence,
        )?;
        self.insert(msg.clone())?;
        Ok(msg)
    }

    fn insert(&self, msg: DelayedMessage) -> Result<(), String> {
        log::debug!(
            "[delayed_send] enqueue creator_tab={:?} creator_label={:?} lead_ms={} id={}",
            msg.created_by_tab,
            msg.created_by_label,
            i128::from(msg.scheduled_at_ms) - i128::from(now_ms()),
            msg.id
        );
        {
            let mut inner = self.inner.lock();
            inner.messages.push(msg);
            sort_messages(&mut inner.messages);
        }
        self.persist()
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_message(
        tab_id: String,
        workspace_id: Option<String>,
        session_id_at_create: Option<String>,
        text: String,
        scheduled_at_ms: Option<u64>,
        creator: ReminderCreator,
        recurrence: Option<RecurrenceRule>,
        team_id: Option<String>,
        last_tick_send_sequence: u64,
    ) -> Result<DelayedMessage, String> {
        if text.trim().is_empty() {
            return Err("Message text is empty".into());
        }
        let (state, scheduled_at_ms) = match scheduled_at_ms {
            Some(ms) => (DelayedMessageState::Queued, ms),
            None => (DelayedMessageState::Stashed, now_ms()),
        };
        if let Some(rule) = &recurrence {
            if state != DelayedMessageState::Queued {
                return Err("recurring messages require a scheduled fire time".into());
            }
            recurrence::validate(rule, scheduled_at_ms)?;
        }
        let msg = DelayedMessage {
            id: format!("dmsg-{}", uuid::Uuid::new_v4().simple()),
            tab_id,
            workspace_id,
            text,
            scheduled_at_ms,
            created_at_ms: now_ms(),
            state,
            error: None,
            session_id_at_create,
            created_by_tab: creator.tab,
            created_by_label: creator.label,
            created_by_model: creator.model,
            recurrence: recurrence.clone(),
            series_start_at_ms: recurrence.map(|_| scheduled_at_ms),
            tick_index: 0,
            skipped_ticks: 0,
            team_id,
            last_tick_unconfirmed: false,
            last_tick_send_sequence,
            origin: MessageOrigin::User,
            reason: None,
            ticks: Vec::new(),
        };
        Ok(msg)
    }

    pub fn update(
        &self,
        id: &str,
        text: Option<String>,
        scheduled_at_ms: Option<u64>,
        stash: Option<bool>,
    ) -> Result<DelayedMessage, String> {
        self.update_with_recurrence(id, text, scheduled_at_ms, stash, None)
    }

    pub fn update_with_recurrence(
        &self,
        id: &str,
        text: Option<String>,
        scheduled_at_ms: Option<u64>,
        stash: Option<bool>,
        recurrence: Option<RecurrenceRule>,
    ) -> Result<DelayedMessage, String> {
        self.update_with_recurrence_at_sequence(id, text, scheduled_at_ms, stash, recurrence, None)
    }

    pub fn update_with_recurrence_at_sequence(
        &self,
        id: &str,
        text: Option<String>,
        scheduled_at_ms: Option<u64>,
        stash: Option<bool>,
        recurrence: Option<RecurrenceRule>,
        last_tick_send_sequence: Option<u64>,
    ) -> Result<DelayedMessage, String> {
        system_entries::refuse_system_mutation(id)?;
        let recurrence_changed = recurrence.is_some();
        let updated = {
            let mut inner = self.inner.lock();
            let index = inner
                .messages
                .iter()
                .position(|msg| msg.id == id)
                .ok_or_else(|| "Delayed message not found".to_string())?;
            if inner.messages[index].state == DelayedMessageState::Sending {
                return Err("Message is currently sending".into());
            }
            let mut updated = inner.messages[index].clone();
            if let Some(text) = text {
                if text.trim().is_empty() {
                    return Err("Message text is empty".into());
                }
                updated.text = text;
            }
            if let Some(scheduled_at_ms) = scheduled_at_ms {
                updated.scheduled_at_ms = scheduled_at_ms;
            }
            if let Some(rule) = recurrence {
                updated.recurrence = Some(rule);
            }
            if stash == Some(true) && scheduled_at_ms.is_none() {
                updated.scheduled_at_ms = now_ms();
            }
            let schedule_changed =
                scheduled_at_ms.is_some() || (stash == Some(true) && scheduled_at_ms.is_none());
            if schedule_changed || recurrence_changed {
                if let Some(rule) = &updated.recurrence {
                    recurrence::validate(rule, updated.scheduled_at_ms)?;
                }
                updated.series_start_at_ms =
                    updated.recurrence.as_ref().map(|_| updated.scheduled_at_ms);
                updated.tick_index = 0;
                updated.skipped_ticks = 0;
                updated.last_tick_unconfirmed = false;
                updated.last_tick_send_sequence = last_tick_send_sequence.unwrap_or_default();
            }
            updated.state = if stash == Some(true) {
                DelayedMessageState::Stashed
            } else {
                DelayedMessageState::Queued
            };
            updated.error = None;
            inner.messages[index] = updated.clone();
            sort_messages(&mut inner.messages);
            updated
        };
        self.persist()?;
        Ok(updated)
    }

    pub fn delete(&self, id: &str) -> Result<DelayedMessage, String> {
        system_entries::refuse_system_mutation(id)?;
        let removed = {
            let mut inner = self.inner.lock();
            let idx = inner
                .messages
                .iter()
                .position(|msg| msg.id == id)
                .ok_or_else(|| "Delayed message not found".to_string())?;
            if inner.messages[idx].state == DelayedMessageState::Sending {
                return Err("Message is currently sending".into());
            }
            inner.messages.remove(idx)
        };
        self.persist()?;
        Ok(removed)
    }

    pub fn claim_due(&self, now_ms: u64) -> Result<Vec<DelayedMessage>, String> {
        let due = {
            let mut inner = self.inner.lock();
            let mut due = Vec::new();
            for msg in &mut inner.messages {
                if system_entries::is_system_entry_id(&msg.id) {
                    continue;
                }
                if msg.state == DelayedMessageState::Queued && msg.scheduled_at_ms <= now_ms {
                    msg.state = DelayedMessageState::Sending;
                    msg.error = None;
                    due.push(msg.clone());
                }
            }
            due
        };
        if !due.is_empty() {
            self.persist()?;
        }
        Ok(due)
    }

    pub fn claim_now(&self, id: &str) -> Result<DelayedMessage, String> {
        system_entries::refuse_system_mutation(id)?;
        let claimed = {
            let mut inner = self.inner.lock();
            let msg = inner
                .messages
                .iter_mut()
                .find(|msg| msg.id == id)
                .ok_or_else(|| "Delayed message not found".to_string())?;
            if msg.state == DelayedMessageState::Sending {
                return Err("Message is currently sending".into());
            }
            msg.state = DelayedMessageState::Sending;
            msg.error = None;
            msg.clone()
        };
        self.persist()?;
        Ok(claimed)
    }

    pub fn mark_sent(&self, id: &str) -> Result<Option<DelayedMessage>, String> {
        let removed = {
            let mut inner = self.inner.lock();
            let Some(idx) = inner.messages.iter().position(|msg| msg.id == id) else {
                return Ok(None);
            };
            inner.messages.remove(idx)
        };
        self.persist()?;
        Ok(Some(removed))
    }

    pub fn finish_tick(
        &self,
        id: &str,
        now_ms: u64,
        error: Option<String>,
        skipped: bool,
    ) -> Result<Option<DelayedMessage>, String> {
        self.finish_tick_at_sequence(id, now_ms, error, skipped, None)
    }

    pub fn finish_tick_at_sequence(
        &self,
        id: &str,
        now_ms: u64,
        error: Option<String>,
        skipped: bool,
        last_tick_send_sequence: Option<u64>,
    ) -> Result<Option<DelayedMessage>, String> {
        let updated = {
            let mut inner = self.inner.lock();
            let Some(index) = inner.messages.iter().position(|msg| msg.id == id) else {
                return Ok(None);
            };
            if inner.messages[index].recurrence.is_none() {
                if error.is_none() {
                    inner.messages.remove(index);
                    None
                } else {
                    let msg = &mut inner.messages[index];
                    msg.state = DelayedMessageState::Failed;
                    msg.error = error;
                    Some(msg.clone())
                }
            } else {
                let rule = inner.messages[index]
                    .recurrence
                    .clone()
                    .ok_or_else(|| "Recurring message lost its recurrence rule".to_string())?;
                let msg = &mut inner.messages[index];
                let start = msg.series_start_at_ms.unwrap_or(msg.scheduled_at_ms);
                let next = recurrence::after_tick(
                    start,
                    &rule,
                    msg.tick_index,
                    now_ms,
                    skipped || error.is_some(),
                );
                if let Some(next_at) = next.scheduled_at_ms {
                    msg.scheduled_at_ms = next_at;
                    msg.tick_index = next.tick_index;
                    msg.skipped_ticks = msg.skipped_ticks.saturating_add(next.skipped_ticks);
                    if let Some(sequence) = last_tick_send_sequence {
                        msg.last_tick_send_sequence = sequence;
                    }
                    msg.state = DelayedMessageState::Queued;
                    msg.error = error.clone();
                    if let Some(error) = error.as_deref() {
                        msg.last_tick_unconfirmed = error.contains("submit_unconfirmed");
                    } else {
                        msg.last_tick_unconfirmed = false;
                    }
                    Some(msg.clone())
                } else {
                    inner.messages.remove(index);
                    None
                }
            }
        };
        self.persist()?;
        Ok(updated)
    }

    pub fn delete_team_series(&self, team_id: &str) -> Result<Vec<DelayedMessage>, String> {
        let removed = {
            let mut inner = self.inner.lock();
            let mut removed = Vec::new();
            inner.messages.retain(|msg| {
                let matches = msg.recurrence.is_some() && msg.team_id.as_deref() == Some(team_id);
                if matches {
                    removed.push(msg.clone());
                }
                !matches
            });
            removed
        };
        if !removed.is_empty() {
            self.persist()?;
        }
        Ok(removed)
    }

    pub fn mark_failed(&self, id: &str, error: String) -> Result<Option<DelayedMessage>, String> {
        let updated = {
            let mut inner = self.inner.lock();
            let Some(msg) = inner.messages.iter_mut().find(|msg| msg.id == id) else {
                return Ok(None);
            };
            msg.state = DelayedMessageState::Failed;
            msg.error = Some(error);
            msg.clone()
        };
        self.persist()?;
        Ok(Some(updated))
    }

    pub fn stop_recurring_for_missing_target(
        &self,
        id: &str,
    ) -> Result<Option<DelayedMessage>, String> {
        let updated = {
            let mut inner = self.inner.lock();
            let Some(msg) = inner.messages.iter_mut().find(|msg| msg.id == id) else {
                return Ok(None);
            };
            if msg.recurrence.is_none() {
                return Ok(None);
            }
            msg.state = DelayedMessageState::Failed;
            msg.error = Some("target tab no longer exists; recurring series stopped".into());
            msg.last_tick_unconfirmed = false;
            msg.clone()
        };
        self.persist()?;
        Ok(Some(updated))
    }

    #[cfg(test)]
    pub(crate) fn refuse_persistence(&self, reason: Option<&str>) {
        *self.persistence_refusal.lock() = reason.map(str::to_owned);
    }

    fn persist(&self) -> Result<(), String> {
        #[cfg(test)]
        if let Some(reason) = self.persistence_refusal.lock().clone() {
            return Err(reason);
        }
        let messages = self.inner.lock().messages.clone();
        let json = serde_json::to_string_pretty(&QueueFile {
            version: QUEUE_VERSION,
            messages,
        })
        .map_err(|e| e.to_string())?;
        queue_file(&self.path).save(&json)
    }
}

/// The strict validator keeps a hand-edited queue whose records no longer all
/// parse: the next save preserves it as a corrupt copy instead of dropping them.
fn queue_file(path: &Path) -> StateFile {
    StateFile::new(path)
        .with_private_permissions()
        .with_validator(validate_queue_strict)
}

fn queue_records(root: &serde_json::Value) -> Result<&Vec<serde_json::Value>, String> {
    root.get("messages")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "queue has no messages array".to_string())
}

fn validate_queue_strict(text: &str) -> Result<(), String> {
    let root: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    for (index, record) in queue_records(&root)?.iter().enumerate() {
        reject_legacy_queue_fields(record).map_err(|e| format!("record {index}: {e}"))?;
        let record = record.clone();
        serde_json::from_value::<DelayedMessage>(record)
            .map_err(|e| format!("record {index}: {e}"))?;
    }
    Ok(())
}

fn parse_queue(path: &Path, text: &str) -> Result<Vec<DelayedMessage>, String> {
    let root: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    Ok(queue_records(&root)?
        .iter()
        .enumerate()
        .filter_map(|(index, record)| parse_queue_record(path, index, record.clone()))
        .collect())
}

fn parse_queue_record(
    path: &Path,
    index: usize,
    record: serde_json::Value,
) -> Option<DelayedMessage> {
    let record_id = record
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<unknown>")
        .to_string();
    if let Err(error) = reject_legacy_queue_fields(&record) {
        log::error!("[delayed_send] quarantined record {record_id} at index {index} in {}: {error}; source left unchanged", path.display());
        return None;
    }
    match serde_json::from_value(record) {
        Ok(message) => Some(message),
        Err(error) => {
            log::error!(
                "[delayed_send] quarantined record {} at index {} in {}: {}; source left unchanged",
                record_id,
                index,
                path.display(),
                error
            );
            None
        }
    }
}

fn reject_legacy_queue_fields(record: &serde_json::Value) -> Result<(), String> {
    for field in ["paneId", "createdByPane"] {
        if record.get(field).is_some() {
            return Err(format!(
                "unsupported legacy delayed-send field {field}; use current tab fields"
            ));
        }
    }
    Ok(())
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn list_views(
    svc: &crate::service::ServiceContext,
    tab_id: Option<String>,
) -> Result<Vec<DelayedMessageView>, String> {
    let resolved = match tab_id {
        Some(tab_id) => Some(resolve_tab_id(svc, &tab_id)?),
        None => None,
    };
    let views: Vec<_> = svc
        .ext::<crate::delayed_send::DelayedSendQueue>()
        .list(resolved.as_deref())
        .into_iter()
        .filter(|msg| {
            msg.state != DelayedMessageState::Sending
                && msg.origin == MessageOrigin::User
                && !system_entries::is_system_entry_id(&msg.id)
        })
        .map(|msg| to_view(svc, msg))
        .collect();
    Ok(views)
}

pub async fn create_message(
    svc: &crate::service::ServiceContext,
    tab_id: String,
    text: String,
    scheduled_at_ms: Option<u64>,
    created_by_tab: Option<String>,
    recurrence: Option<RecurrenceRule>,
) -> Result<DelayedMessageView, String> {
    create_message_with_delivery(
        svc,
        tab_id,
        text,
        scheduled_at_ms,
        created_by_tab,
        recurrence,
        now_ms(),
        |service, message| async move { delivery::deliver(&service, &message).await },
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn create_message_with_delivery<F, Fut>(
    svc: &crate::service::ServiceContext,
    tab_id: String,
    text: String,
    scheduled_at_ms: Option<u64>,
    created_by_tab: Option<String>,
    recurrence: Option<RecurrenceRule>,
    now: u64,
    deliver: F,
) -> Result<DelayedMessageView, String>
where
    F: FnOnce(ServiceState, DelayedMessage) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let resolved = resolve_tab_id(svc, &tab_id)?;
    let (workspace_id, session_id_at_create) = tab_context(svc, &resolved);
    let team_source = created_by_tab
        .as_deref()
        .and_then(|creator| resolve_tab_id(svc, creator).ok())
        .unwrap_or_else(|| resolved.clone());
    let team_id = svc
        .mux
        .read()
        .get_tab(&team_source)
        .and_then(|tab| tab.team_id.clone());
    let creator = resolve_creator(svc, created_by_tab);
    let last_tick_send_sequence = latest_ledger_sequence(svc, &resolved);
    let mut msg = DelayedSendQueue::prepare_message(
        resolved.clone(),
        workspace_id,
        session_id_at_create,
        text,
        scheduled_at_ms,
        creator,
        recurrence,
        team_id,
        last_tick_send_sequence,
    )?;
    if scheduled_at_ms.is_some_and(|at| at <= now.saturating_add(1_000)) {
        msg.state = DelayedMessageState::Sending;
        let service = svc
            .weak_self()
            .upgrade()
            .ok_or("delivery service is unavailable")?;
        let result = deliver(service, msg.clone()).await;
        if let Some(rule) = &msg.recurrence {
            let next = recurrence::after_tick(
                msg.scheduled_at_ms,
                rule,
                0,
                now_ms().saturating_add(1),
                result.is_err(),
            );
            if let Some(at) = next.scheduled_at_ms {
                let mut future = msg.clone();
                future.scheduled_at_ms = at;
                future.tick_index = next.tick_index;
                future.skipped_ticks = next.skipped_ticks;
                future.state = DelayedMessageState::Queued;
                future.error = result.as_ref().err().cloned();
                future.last_tick_unconfirmed = future
                    .error
                    .as_deref()
                    .is_some_and(|error| error.contains("submit_unconfirmed"));
                svc.ext::<DelayedSendQueue>().insert(future)?;
                emit_changed(svc, &resolved);
            }
        }
        result?;
    } else {
        svc.ext::<DelayedSendQueue>().insert(msg.clone())?;
        emit_changed(svc, &resolved);
    }
    Ok(to_view(svc, msg))
}

fn resolve_creator(
    svc: &crate::service::ServiceContext,
    created_by_tab: Option<String>,
) -> ReminderCreator {
    let Some(raw) = created_by_tab.filter(|p| !p.trim().is_empty()) else {
        return ReminderCreator::default();
    };
    let mux = svc.mux.read();
    let tab = mux.resolve_tab_id(&raw).unwrap_or(raw);
    let label = mux.display_label_for_tab(&tab);
    let model = mux
        .get_tab(&tab)
        .and_then(|p| p.agent_session_model.clone());
    ReminderCreator {
        tab: Some(tab),
        label: Some(label).filter(|s| !s.trim().is_empty()),
        model,
    }
}

pub fn update_message(
    svc: &crate::service::ServiceContext,
    id: String,
    text: Option<String>,
    scheduled_at_ms: Option<u64>,
    stash: Option<bool>,
    recurrence: Option<RecurrenceRule>,
) -> Result<DelayedMessageView, String> {
    let last_tick_send_sequence = svc
        .ext::<crate::delayed_send::DelayedSendQueue>()
        .list(None)
        .iter()
        .find(|message| message.id == id)
        .map(|message| latest_ledger_sequence(svc, &message.tab_id));
    let msg = svc
        .ext::<crate::delayed_send::DelayedSendQueue>()
        .update_with_recurrence_at_sequence(
            &id,
            text,
            scheduled_at_ms,
            stash,
            recurrence,
            last_tick_send_sequence,
        )?;
    emit_changed(svc, &msg.tab_id);
    Ok(to_view(svc, msg))
}

pub fn delete_message(
    svc: &crate::service::ServiceContext,
    id: String,
) -> Result<DelayedMessageView, String> {
    let msg = svc
        .ext::<crate::delayed_send::DelayedSendQueue>()
        .delete(&id)?;
    emit_changed(svc, &msg.tab_id);
    Ok(to_view(svc, msg))
}

pub fn delete_team_series(
    svc: &crate::service::ServiceContext,
    team_id: &str,
) -> Result<(), String> {
    for msg in svc
        .ext::<crate::delayed_send::DelayedSendQueue>()
        .delete_team_series(team_id)?
    {
        emit_changed(svc, &msg.tab_id);
    }
    Ok(())
}

fn latest_ledger_sequence(svc: &crate::service::ServiceContext, tab_id: &str) -> u64 {
    svc.chat_send_ledger
        .reconcile(tab_id)
        .iter()
        .map(|send| send.sequence)
        .max()
        .unwrap_or_default()
}

fn has_unprocessed_send_after_tick(
    sends: &[crate::service::chat_send_transaction::ChatSendStatus],
    last_tick_send_sequence: u64,
) -> bool {
    sends.iter().any(|send| {
        send.sequence > last_tick_send_sequence
            && matches!(
                send.state,
                crate::service::chat_send_transaction::ChatSendState::Pending
                    | crate::service::chat_send_transaction::ChatSendState::Written
                    | crate::service::chat_send_transaction::ChatSendState::Unknown
                    | crate::service::chat_send_transaction::ChatSendState::Failed
            )
    })
}

pub async fn send_message_now(svc: ServiceState, id: String) -> Result<(), String> {
    let msg = svc
        .ext::<crate::delayed_send::DelayedSendQueue>()
        .claim_now(&id)?;
    let tab_id = msg.tab_id.clone();
    let result = deliver_claimed(&svc, msg).await;
    emit_changed(&svc, &tab_id);
    result
}

pub fn start_scheduler(svc: ServiceState) {
    crate::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let due = match svc
                .ext::<crate::delayed_send::DelayedSendQueue>()
                .claim_due(now_ms())
            {
                Ok(due) => due,
                Err(e) => {
                    log::warn!("[delayed_send] claim due failed: {}", e);
                    continue;
                }
            };
            for msg in due {
                let tab_id = msg.tab_id.clone();
                if msg.recurrence.is_some() && !target_tab_exists(&svc, &tab_id) {
                    match svc
                        .ext::<crate::delayed_send::DelayedSendQueue>()
                        .stop_recurring_for_missing_target(&msg.id)
                    {
                        Ok(Some(_)) => log::warn!(
                            "[delayed_send] stopped recurring series id={} tab={}: target tab no longer exists",
                            msg.id,
                            tab_id
                        ),
                        Ok(None) => {}
                        Err(error) => log::warn!(
                            "[delayed_send] failed to stop recurring series id={}: {}",
                            msg.id,
                            error
                        ),
                    }
                    emit_changed(&svc, &tab_id);
                    continue;
                }
                if msg.recurrence.is_some() && target_has_unprocessed_tick(&svc, &msg) {
                    let now = now_ms();
                    let last_tick_send_sequence = latest_ledger_sequence(&svc, &tab_id);
                    let skipped_total = msg
                        .recurrence
                        .as_ref()
                        .map(|rule| {
                            recurrence::after_tick(
                                msg.series_start_at_ms.unwrap_or(msg.scheduled_at_ms),
                                rule,
                                msg.tick_index,
                                now,
                                true,
                            )
                            .skipped_ticks
                        })
                        .unwrap_or(1)
                        .saturating_add(msg.skipped_ticks);
                    match svc
                        .ext::<crate::delayed_send::DelayedSendQueue>()
                        .finish_tick_at_sequence(
                            &msg.id,
                            now,
                            None,
                            true,
                            Some(last_tick_send_sequence),
                        ) {
                        Ok(_) => log::info!(
                            "[delayed_send] skipped recurring tick id={} tab={} skipped={}",
                            msg.id,
                            tab_id,
                            skipped_total
                        ),
                        Err(error) => log::warn!(
                            "[delayed_send] recurring tick skip failed id={}: {}",
                            msg.id,
                            error
                        ),
                    }
                    emit_changed(&svc, &tab_id);
                    continue;
                }
                if let Err(e) = deliver_claimed(&svc, msg).await {
                    log::warn!("[delayed_send] delivery failed for Tab {}: {}", tab_id, e);
                }
                emit_changed(&svc, &tab_id);
            }
        }
    });
}

pub(crate) async fn deliver_claimed(svc: &ServiceState, msg: DelayedMessage) -> Result<(), String> {
    if !target_tab_exists(svc, &msg.tab_id) {
        let error = format!("recipient tab {} no longer exists", msg.tab_id);
        if msg.recurrence.is_some() {
            svc.ext::<crate::delayed_send::DelayedSendQueue>()
                .stop_recurring_for_missing_target(&msg.id)?;
        } else {
            svc.ext::<crate::delayed_send::DelayedSendQueue>()
                .mark_failed(&msg.id, error.clone())?;
        }
        return Err(error);
    }
    let last_tick_send_sequence = latest_ledger_sequence(svc, &msg.tab_id);
    let tab_id = msg.tab_id.clone();
    let result = delivery::deliver(svc, &msg).await;
    match result {
        Ok(_) => {
            svc.ext::<crate::delayed_send::DelayedSendQueue>()
                .finish_tick_at_sequence(
                    &msg.id,
                    now_ms(),
                    None,
                    false,
                    Some(last_tick_send_sequence),
                )?;
            Ok(())
        }
        Err(e) => {
            if msg.recurrence.is_some() && !target_tab_exists(svc, &tab_id) {
                svc.ext::<crate::delayed_send::DelayedSendQueue>()
                    .stop_recurring_for_missing_target(&msg.id)?;
                log::warn!(
                    "[delayed_send] stopped recurring series id={} tab={}: target tab no longer exists",
                    msg.id,
                    tab_id
                );
            } else {
                svc.ext::<crate::delayed_send::DelayedSendQueue>()
                    .finish_tick_at_sequence(
                        &msg.id,
                        now_ms(),
                        Some(e.clone()),
                        false,
                        Some(last_tick_send_sequence),
                    )?;
            }
            Err(e)
        }
    }
}

fn target_tab_exists(svc: &ServiceState, tab_id: &str) -> bool {
    svc.mux.read().get_tab(tab_id).is_some()
}

fn target_has_unprocessed_tick(svc: &ServiceState, msg: &DelayedMessage) -> bool {
    let sends = svc.chat_send_ledger.reconcile(&msg.tab_id);
    let unresolved_send = has_unprocessed_send_after_tick(&sends, msg.last_tick_send_sequence);
    let (pty_id, pending_delivery) = {
        let mux = svc.mux.read();
        let Some(tab) = mux.get_tab(&msg.tab_id) else {
            return false;
        };
        (
            tab.pty_id.clone(),
            tab.pending_delivery
                .as_ref()
                .is_some_and(|pending| pending.state().needs_delivery()),
        )
    };
    let tab_busy = pty_id.is_some_and(|pty_id| {
        svc.agent_tracker
            .lock()
            .get_status_if_tracked(&pty_id)
            .is_some_and(|status| status != crate::agent::status::TabStatus::Idle)
    });
    recurrence::should_skip_tick(
        msg.last_tick_unconfirmed,
        pending_delivery,
        unresolved_send,
        tab_busy,
    )
}

fn reminder_envelope(msg: &DelayedMessage) -> crate::service::messenger::MessageEnvelope {
    let envelope = crate::service::messenger::MessageEnvelope::reminder(
        msg.created_by_label.clone().unwrap_or_default(),
        msg.created_by_tab.clone().unwrap_or_default(),
    )
    .model(msg.created_by_model.clone())
    .set_at(msg.created_at_ms)
    .fire_at(msg.scheduled_at_ms)
    .id(Some(msg.id.clone()));
    let Some(rule) = &msg.recurrence else {
        return envelope;
    };
    let start = msg.series_start_at_ms.unwrap_or(msg.scheduled_at_ms);
    let next = recurrence::after_tick(start, rule, msg.tick_index, now_ms(), false);
    if next.scheduled_at_ms.is_some() {
        envelope.recurrence(
            rule.every_ms,
            next.scheduled_at_ms,
            msg.skipped_ticks.saturating_add(next.skipped_ticks),
        )
    } else {
        envelope
    }
}

fn resolve_tab_id(
    svc: &crate::service::ServiceContext,
    id_or_alias: &str,
) -> Result<String, String> {
    svc.mux
        .read()
        .resolve_tab_id(id_or_alias)
        .ok_or_else(|| "Tab not found".to_string())
}

fn tab_context(
    svc: &crate::service::ServiceContext,
    tab_id: &str,
) -> (Option<String>, Option<String>) {
    let mux = svc.mux.read();
    let workspace_id = mux.find_workspace_for_tab(tab_id);
    let session_id = mux
        .get_tab(tab_id)
        .and_then(|tab| tab.agent_session_id.clone());
    (workspace_id, session_id)
}

fn to_view(svc: &crate::service::ServiceContext, msg: DelayedMessage) -> DelayedMessageView {
    let current_session_id = svc
        .mux
        .read()
        .get_tab(&msg.tab_id)
        .and_then(|tab| tab.agent_session_id.clone());
    let session_changed = match (&msg.session_id_at_create, &current_session_id) {
        (Some(a), Some(b)) => a != b,
        (Some(_), None) => true,
        _ => false,
    };
    DelayedMessageView {
        message: msg,
        current_session_id,
        session_changed,
    }
}

pub(crate) fn emit_changed(svc: &crate::service::ServiceContext, tab_id: &str) {
    svc.ui.emit(
        "delayed-send-changed",
        serde_json::to_value(serde_json::json!({ "tabId": tab_id })).unwrap_or_default(),
    );
}

fn sort_messages(messages: &mut [DelayedMessage]) {
    messages.sort_by(|a, b| {
        let a_stashed = a.state == DelayedMessageState::Stashed;
        let b_stashed = b.state == DelayedMessageState::Stashed;
        a_stashed
            .cmp(&b_stashed)
            .then_with(|| a.scheduled_at_ms.cmp(&b.scheduled_at_ms))
            .then_with(|| a.created_at_ms.cmp(&b.created_at_ms))
            .then_with(|| a.id.cmp(&b.id))
    });
}

#[cfg(test)]
mod tests;
