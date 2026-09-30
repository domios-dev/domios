//! Unread sticky notifications outlive the daemon. Only records that still wait
//! on the user are written; everything else stays session-scoped.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use codeterm_types::notification::{AnswerSlot, NotificationAction};
use codeterm_types::pty::PromptIdentity;

use super::channel_placement::ChannelPlacement;
use super::dedupe::TabCondition;
use super::push::NotifyLevel;
use super::store::{Notification, NotificationSeverity};

pub const FILE_NAME: &str = "notifications.json";

/// Mirror of a stored action, read back only from the daemon's own file. The
/// caller-facing parse seams never see this type.
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum StoredAction {
    Url {
        label: String,
        value: String,
    },
    Dismiss {
        label: String,
    },
    Answer {
        label: String,
    },
    RespondPromptOption {
        label: String,
        value: String,
        identity: PromptIdentity,
    },
}

impl From<&NotificationAction> for StoredAction {
    fn from(action: &NotificationAction) -> Self {
        match action.clone() {
            NotificationAction::Url { label, value } => Self::Url { label, value },
            NotificationAction::Dismiss { label } => Self::Dismiss { label },
            NotificationAction::Answer { label } => Self::Answer { label },
            NotificationAction::RespondPromptOption {
                label,
                value,
                identity,
            } => Self::RespondPromptOption {
                label,
                value,
                identity,
            },
        }
    }
}

impl From<StoredAction> for NotificationAction {
    fn from(action: StoredAction) -> Self {
        match action {
            StoredAction::Url { label, value } => Self::Url { label, value },
            StoredAction::Dismiss { label } => Self::Dismiss { label },
            StoredAction::Answer { label } => Self::Answer { label },
            StoredAction::RespondPromptOption {
                label,
                value,
                identity,
            } => Self::RespondPromptOption {
                label,
                value,
                identity,
            },
        }
    }
}

#[derive(Serialize, Deserialize)]
struct StoredNotification {
    id: String,
    tab_id: Option<String>,
    workspace_id: Option<String>,
    tab_title: Option<String>,
    peer_name: Option<String>,
    target_tab_id: Option<String>,
    chat_message_id: Option<String>,
    title: String,
    body: Option<String>,
    severity: NotificationSeverity,
    level: Option<NotifyLevel>,
    timestamp: u64,
    actions: Option<Vec<StoredAction>>,
    answer: Option<AnswerSlot>,
    sticky: Option<bool>,
    #[serde(default)]
    condition: Option<TabCondition>,
    #[serde(default)]
    placements: Vec<ChannelPlacement>,
}

/// Whether a record still waits on the user across a restart. An offer is an
/// authority grant, and a restarted daemon never re-mints one from disk.
pub fn outlives_restart(notification: &Notification) -> bool {
    !notification.read
        && !notification.resolved
        && notification.sticky == Some(true)
        && notification.surface_offer.is_none()
}

fn stored(notification: &Notification) -> StoredNotification {
    StoredNotification {
        id: notification.id.clone(),
        tab_id: notification.tab_id.clone(),
        workspace_id: notification.workspace_id.clone(),
        tab_title: notification.tab_title.clone(),
        peer_name: notification.peer_name.clone(),
        target_tab_id: notification.target_tab_id.clone(),
        chat_message_id: notification.chat_message_id.clone(),
        title: notification.title.clone(),
        body: notification.body.clone(),
        severity: notification.severity.clone(),
        level: notification.level,
        timestamp: notification.timestamp,
        actions: notification
            .actions
            .as_ref()
            .map(|actions| actions.iter().map(StoredAction::from).collect()),
        answer: notification.answer.clone(),
        sticky: notification.sticky,
        condition: notification.condition,
        placements: notification.placements.clone(),
    }
}

fn restored(record: StoredNotification) -> Notification {
    Notification {
        id: record.id,
        tab_id: record.tab_id,
        workspace_id: record.workspace_id,
        tab_title: record.tab_title,
        peer_name: record.peer_name,
        target_tab_id: record.target_tab_id,
        chat_message_id: record.chat_message_id,
        title: record.title,
        body: record.body,
        severity: record.severity,
        level: record.level,
        timestamp: record.timestamp,
        read: false,
        actions: record
            .actions
            .map(|actions| actions.into_iter().map(NotificationAction::from).collect()),
        answer: record.answer,
        resolved: false,
        surface_offer: None,
        sticky: record.sticky,
        push_failed: None,
        placements: record.placements,
        condition: record.condition,
    }
}

fn render(notifications: &[Notification]) -> String {
    let records: Vec<StoredNotification> = notifications
        .iter()
        .filter(|notification| outlives_restart(notification))
        .map(stored)
        .collect();
    serde_json::to_string_pretty(&records).unwrap_or_else(|_| "[]".to_string())
}

/// The on-disk copy of the records that outlive a restart. Written only when
/// that set changes, so ordinary notifications cost no IO.
pub struct Persistence {
    path: PathBuf,
    written: String,
}

impl Persistence {
    /// Load what the previous daemon left. An unreadable file is kept beside
    /// the fresh one rather than overwritten, so nothing is silently lost.
    pub fn open(path: PathBuf) -> (Self, Vec<Notification>) {
        let (written, records) = match std::fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Vec<StoredNotification>>(&text) {
                Ok(records) => (text, records.into_iter().map(restored).collect()),
                Err(error) => {
                    set_aside_unreadable(&path, &error.to_string());
                    (String::new(), Vec::new())
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (render(&[]), Vec::new()),
            Err(error) => {
                log::warn!(
                    "[notify-persist] {} could not be read: {error}",
                    path.display()
                );
                (String::new(), Vec::new())
            }
        };
        (Self { path, written }, records)
    }

    pub fn sync(&mut self, notifications: &[Notification]) {
        let rendered = render(notifications);
        if rendered == self.written {
            return;
        }
        match codeterm_util::atomic_write::write_atomic(&self.path, rendered.as_bytes()) {
            Ok(()) => self.written = rendered,
            Err(error) => log::warn!(
                "[notify-persist] {} could not be written: {error}",
                self.path.display()
            ),
        }
    }
}

fn set_aside_unreadable(path: &Path, reason: &str) {
    let aside = path.with_extension("json.unreadable");
    match std::fs::rename(path, &aside) {
        Ok(()) => log::warn!(
            "[notify-persist] {} is unreadable ({reason}); kept as {}",
            path.display(),
            aside.display()
        ),
        Err(error) => log::warn!(
            "[notify-persist] {} is unreadable ({reason}) and could not be set aside: {error}",
            path.display()
        ),
    }
}
