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
    stash_at(&path(), text, sender)
}

fn stash_at(p: &Path, text: &str, sender: Option<TurnResumeSender>) -> std::io::Result<()> {
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let message = PendingMessage {
        text: text.to_string(),
        sender,
    };
    let serialized = serde_json::to_vec(&message).map_err(std::io::Error::other)?;
    codeterm_util::atomic_write::write_atomic(p, serialized)
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
mod tests;
