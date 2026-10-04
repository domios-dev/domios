use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::multiplexer::types::TurnResumeSender;

const FILENAME: &str = "post-update-message.txt";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PendingMessage {
    pub text: String,
    pub sender: Option<TurnResumeSender>,
}

fn path() -> PathBuf {
    crate::paths::data_dir().join(FILENAME)
}

pub fn stash(text: &str, sender: Option<TurnResumeSender>) -> std::io::Result<()> {
    let p = path();
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let message = PendingMessage {
        text: text.to_string(),
        sender,
    };
    let serialized = serde_json::to_vec(&message).map_err(std::io::Error::other)?;
    std::fs::write(p, serialized)
}

pub(crate) fn pending() -> Option<PendingMessage> {
    pending_at(&path())
}

pub(crate) fn pending_at(path: &Path) -> Option<PendingMessage> {
    let raw = std::fs::read_to_string(path).ok()?;
    parse_pending(&raw)
}

fn parse_pending(raw: &str) -> Option<PendingMessage> {
    if let Ok(message) = serde_json::from_str::<PendingMessage>(raw) {
        return (!message.text.trim().is_empty()).then_some(message);
    }
    (!raw.trim().is_empty()).then_some(PendingMessage {
        text: raw.to_string(),
        sender: None,
    })
}

pub(crate) fn consume() -> std::io::Result<()> {
    match std::fs::remove_file(path()) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_message_round_trips_requester_identity() {
        let message = PendingMessage {
            text: "opaque update request".into(),
            sender: Some(TurnResumeSender {
                tab_id: "tab-a".into(),
                label: "Agent A".into(),
                from_agent: Some(true),
                from_model: Some("model-a".into()),
            }),
        };

        let serialized = serde_json::to_string(&message).unwrap();
        let restored: PendingMessage = serde_json::from_str(&serialized).unwrap();

        assert_eq!(restored, message);
    }

    #[test]
    fn prior_plain_text_stashes_remain_readable() {
        let restored = parse_pending("opaque legacy text").unwrap();

        assert_eq!(restored.text, "opaque legacy text");
        assert!(restored.sender.is_none());
    }

    #[test]
    fn pending_reads_the_stashed_message_from_its_owned_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILENAME);
        let message = PendingMessage {
            text: "opaque update prompt".into(),
            sender: None,
        };
        std::fs::write(&path, serde_json::to_vec(&message).unwrap()).unwrap();

        assert_eq!(pending_at(&path), Some(message));
    }
}
