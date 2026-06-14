use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tauri::Emitter;

use crate::service::ServiceState;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelayedMessageState {
    Queued,
    Sending,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelayedMessage {
    pub id: String,
    pub pane_id: String,
    pub workspace_id: Option<String>,
    pub text: String,
    pub scheduled_at_ms: u64,
    pub created_at_ms: u64,
    pub state: DelayedMessageState,
    pub error: Option<String>,
    pub session_id_at_create: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelayedMessageView {
    #[serde(flatten)]
    pub message: DelayedMessage,
    pub current_session_id: Option<String>,
    pub session_changed: bool,
}

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
}

impl DelayedSendQueue {
    pub fn open(path: PathBuf) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("create {}: {}", parent.display(), e))?;
        }
        let mut file = if path.exists() {
            let bytes = fs::read(&path).map_err(|e| format!("read {}: {}", path.display(), e))?;
            serde_json::from_slice::<QueueFile>(&bytes)
                .map_err(|e| format!("parse {}: {}", path.display(), e))?
        } else {
            QueueFile {
                version: 1,
                messages: Vec::new(),
            }
        };
        for msg in &mut file.messages {
            if msg.state == DelayedMessageState::Sending {
                msg.state = DelayedMessageState::Queued;
            }
        }
        sort_messages(&mut file.messages);
        let queue = Self {
            path,
            inner: Arc::new(Mutex::new(QueueInner {
                messages: file.messages,
            })),
        };
        queue.persist()?;
        Ok(queue)
    }

    pub fn list(&self, pane_id: Option<&str>) -> Vec<DelayedMessage> {
        let mut messages: Vec<_> = self
            .inner
            .lock()
            .messages
            .iter()
            .filter(|msg| pane_id.map_or(true, |pane_id| msg.pane_id == pane_id))
            .cloned()
            .collect();
        sort_messages(&mut messages);
        messages
    }

    pub fn enqueue(
        &self,
        pane_id: String,
        workspace_id: Option<String>,
        session_id_at_create: Option<String>,
        text: String,
        scheduled_at_ms: u64,
    ) -> Result<DelayedMessage, String> {
        if text.trim().is_empty() {
            return Err("Message text is empty".into());
        }
        let msg = DelayedMessage {
            id: format!("dmsg-{}", uuid::Uuid::new_v4().simple()),
            pane_id,
            workspace_id,
            text,
            scheduled_at_ms,
            created_at_ms: now_ms(),
            state: DelayedMessageState::Queued,
            error: None,
            session_id_at_create,
        };
        {
            let mut inner = self.inner.lock();
            inner.messages.push(msg.clone());
            sort_messages(&mut inner.messages);
        }
        self.persist()?;
        Ok(msg)
    }

    pub fn update(
        &self,
        id: &str,
        text: Option<String>,
        scheduled_at_ms: Option<u64>,
    ) -> Result<DelayedMessage, String> {
        let updated = {
            let mut inner = self.inner.lock();
            let msg = inner
                .messages
                .iter_mut()
                .find(|msg| msg.id == id)
                .ok_or_else(|| "Delayed message not found".to_string())?;
            if msg.state == DelayedMessageState::Sending {
                return Err("Message is currently sending".into());
            }
            if let Some(text) = text {
                if text.trim().is_empty() {
                    return Err("Message text is empty".into());
                }
                msg.text = text;
            }
            if let Some(scheduled_at_ms) = scheduled_at_ms {
                msg.scheduled_at_ms = scheduled_at_ms;
            }
            msg.state = DelayedMessageState::Queued;
            msg.error = None;
            let updated = msg.clone();
            sort_messages(&mut inner.messages);
            updated
        };
        self.persist()?;
        Ok(updated)
    }

    pub fn delete(&self, id: &str) -> Result<DelayedMessage, String> {
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

    fn persist(&self) -> Result<(), String> {
        let messages = self.inner.lock().messages.clone();
        write_queue_file(
            &self.path,
            &QueueFile {
                version: 1,
                messages,
            },
        )
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn list_views(
    svc: &ServiceState,
    pane_id: Option<String>,
) -> Result<Vec<DelayedMessageView>, String> {
    let resolved = match pane_id {
        Some(pane_id) => Some(resolve_pane_id(svc, &pane_id)?),
        None => None,
    };
    Ok(svc
        .delayed_send
        .list(resolved.as_deref())
        .into_iter()
        .map(|msg| to_view(svc, msg))
        .collect())
}

pub fn create_message(
    svc: &ServiceState,
    pane_id: String,
    text: String,
    scheduled_at_ms: u64,
) -> Result<DelayedMessageView, String> {
    let resolved = resolve_pane_id(svc, &pane_id)?;
    let (workspace_id, session_id_at_create) = pane_context(svc, &resolved);
    let msg = svc.delayed_send.enqueue(
        resolved.clone(),
        workspace_id,
        session_id_at_create,
        text,
        scheduled_at_ms,
    )?;
    emit_changed(svc, &resolved);
    Ok(to_view(svc, msg))
}

pub fn update_message(
    svc: &ServiceState,
    id: String,
    text: Option<String>,
    scheduled_at_ms: Option<u64>,
) -> Result<DelayedMessageView, String> {
    let msg = svc.delayed_send.update(&id, text, scheduled_at_ms)?;
    emit_changed(svc, &msg.pane_id);
    Ok(to_view(svc, msg))
}

pub fn delete_message(svc: &ServiceState, id: String) -> Result<DelayedMessageView, String> {
    let msg = svc.delayed_send.delete(&id)?;
    emit_changed(svc, &msg.pane_id);
    Ok(to_view(svc, msg))
}

pub async fn send_message_now(svc: ServiceState, id: String) -> Result<(), String> {
    let msg = svc.delayed_send.claim_now(&id)?;
    let pane_id = msg.pane_id.clone();
    let result = deliver_claimed(&svc, msg).await;
    emit_changed(&svc, &pane_id);
    result
}

pub fn start_scheduler(svc: ServiceState) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let due = match svc.delayed_send.claim_due(now_ms()) {
                Ok(due) => due,
                Err(e) => {
                    log::warn!("[delayed_send] claim due failed: {}", e);
                    continue;
                }
            };
            for msg in due {
                let pane_id = msg.pane_id.clone();
                if let Err(e) = deliver_claimed(&svc, msg).await {
                    log::warn!("[delayed_send] delivery failed for pane {}: {}", pane_id, e);
                }
                emit_changed(&svc, &pane_id);
            }
        }
    });
}

async fn deliver_claimed(svc: &ServiceState, msg: DelayedMessage) -> Result<(), String> {
    let result = svc
        .send_text_when_ready(&msg.pane_id, &msg.text, true)
        .await;
    match result {
        Ok(()) => {
            svc.delayed_send.mark_sent(&msg.id)?;
            Ok(())
        }
        Err(e) => {
            let _ = svc.delayed_send.mark_failed(&msg.id, e.clone())?;
            Err(e)
        }
    }
}

fn resolve_pane_id(svc: &ServiceState, id_or_alias: &str) -> Result<String, String> {
    svc.mux
        .lock()
        .resolve_pane_id(id_or_alias)
        .ok_or_else(|| "Pane not found".to_string())
}

fn pane_context(svc: &ServiceState, pane_id: &str) -> (Option<String>, Option<String>) {
    let mux = svc.mux.lock();
    let workspace_id = mux.find_workspace_for_pane(pane_id);
    let session_id = mux
        .get_pane(pane_id)
        .and_then(|pane| pane.agent_session_id.clone());
    (workspace_id, session_id)
}

fn to_view(svc: &ServiceState, msg: DelayedMessage) -> DelayedMessageView {
    let current_session_id = svc
        .mux
        .lock()
        .get_pane(&msg.pane_id)
        .and_then(|pane| pane.agent_session_id.clone());
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

fn emit_changed(svc: &ServiceState, pane_id: &str) {
    let _ = svc.app_handle.emit(
        "delayed-send-changed",
        serde_json::json!({ "paneId": pane_id }),
    );
}

fn write_queue_file(path: &Path, file: &QueueFile) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("create {}: {}", parent.display(), e))?;
    }
    let bytes = serde_json::to_vec_pretty(file).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, bytes).map_err(|e| format!("write {}: {}", tmp.display(), e))?;
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(first) if path.exists() => {
            fs::remove_file(path)
                .map_err(|e| format!("replace {}: {}; {}", path.display(), first, e))?;
            fs::rename(&tmp, path).map_err(|e| format!("rename {}: {}", path.display(), e))
        }
        Err(e) => Err(format!("rename {}: {}", path.display(), e)),
    }
}

fn sort_messages(messages: &mut [DelayedMessage]) {
    messages.sort_by(|a, b| {
        a.scheduled_at_ms
            .cmp(&b.scheduled_at_ms)
            .then_with(|| a.created_at_ms.cmp(&b.created_at_ms))
            .then_with(|| a.id.cmp(&b.id))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queue() -> (tempfile::TempDir, DelayedSendQueue) {
        let dir = tempfile::tempdir().unwrap();
        let queue = DelayedSendQueue::open(dir.path().join("delayed_send.json")).unwrap();
        (dir, queue)
    }

    #[test]
    fn persists_and_restores_messages() {
        let (dir, queue) = queue();
        let msg = queue
            .enqueue(
                "pane-a".into(),
                Some("ws".into()),
                Some("sid-1".into()),
                "hello".into(),
                100,
            )
            .unwrap();
        drop(queue);

        let reopened = DelayedSendQueue::open(dir.path().join("delayed_send.json")).unwrap();
        let messages = reopened.list(Some("pane-a"));
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].id, msg.id);
        assert_eq!(messages[0].text, "hello");
        assert_eq!(messages[0].state, DelayedMessageState::Queued);
    }

    #[test]
    fn claim_due_uses_schedule_then_creation_order() {
        let (_dir, queue) = queue();
        let second = queue
            .enqueue("pane".into(), None, None, "second".into(), 100)
            .unwrap();
        let first = queue
            .enqueue("pane".into(), None, None, "first".into(), 50)
            .unwrap();
        let due = queue.claim_due(100).unwrap();
        assert_eq!(
            due.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec![first.id.as_str(), second.id.as_str(),]
        );
        assert!(queue
            .list(Some("pane"))
            .iter()
            .all(|msg| msg.state == DelayedMessageState::Sending));
    }

    #[test]
    fn failed_message_can_be_updated_back_to_queued() {
        let (_dir, queue) = queue();
        let msg = queue
            .enqueue("pane".into(), None, None, "old".into(), 100)
            .unwrap();
        queue.mark_failed(&msg.id, "missing pane".into()).unwrap();
        let updated = queue
            .update(&msg.id, Some("new".into()), Some(200))
            .unwrap();
        assert_eq!(updated.text, "new");
        assert_eq!(updated.scheduled_at_ms, 200);
        assert_eq!(updated.state, DelayedMessageState::Queued);
        assert_eq!(updated.error, None);
    }

    #[test]
    fn sending_messages_reset_to_queued_on_open() {
        let (dir, queue) = queue();
        let msg = queue
            .enqueue("pane".into(), None, None, "hello".into(), 100)
            .unwrap();
        queue.claim_now(&msg.id).unwrap();
        drop(queue);

        let reopened = DelayedSendQueue::open(dir.path().join("delayed_send.json")).unwrap();
        assert_eq!(
            reopened.list(Some("pane"))[0].state,
            DelayedMessageState::Queued
        );
    }
}
