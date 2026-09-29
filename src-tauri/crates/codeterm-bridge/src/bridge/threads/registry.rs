//! Durable tab/chat/thread bindings. Stores ids and topic names only: never a
//! token, URL, port or any other connection value.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use codeterm_util::persisted_state::{Loaded, StateFile};

use serde::{Deserialize, Serialize};

pub const MAX_MESSAGE_ORIGINS: usize = 256;
const MAX_CLOSED_BINDINGS: usize = 64;

/// One bot's conversation with one chat.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChatScope {
    pub service: String,
    pub bot_id: i64,
    pub chat_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadBinding {
    pub scope: ChatScope,
    pub tab_id: String,
    pub thread_id: i64,
    pub topic_name: String,
    #[serde(default)]
    pub intro_posted: bool,
    /// Deletion failed and the topic was renamed; it routes to the General Agent.
    #[serde(default)]
    pub closed: bool,
    /// A send found the topic missing or closed; the next attach verifies it before reuse.
    #[serde(default)]
    pub stale: bool,
}

/// A message a tab placed, so a reply to it can find the tab without topics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageOrigin {
    pub scope: ChatScope,
    pub message_id: i64,
    pub tab_id: String,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct Snapshot {
    #[serde(default)]
    bindings: Vec<ThreadBinding>,
    #[serde(default)]
    topics_notice_sent: Vec<ChatScope>,
    #[serde(default)]
    origins: Vec<MessageOrigin>,
}

pub struct ThreadRegistry {
    path: Option<PathBuf>,
    snapshot: Snapshot,
    /// Per bridge run: a restart probes topic support again.
    topics_unavailable: HashSet<ChatScope>,
}

impl ThreadRegistry {
    pub fn in_memory() -> Self {
        Self {
            path: None,
            snapshot: Snapshot::default(),
            topics_unavailable: HashSet::new(),
        }
    }

    pub fn load(path: PathBuf) -> Self {
        let snapshot = match state_file(&path).load(parse_snapshot) {
            Loaded::Parsed(snapshot)
            | Loaded::Recovered {
                value: snapshot, ..
            } => snapshot,
            Loaded::Missing | Loaded::Unrecoverable { .. } => Snapshot::default(),
        };
        Self {
            path: Some(path),
            snapshot,
            topics_unavailable: HashSet::new(),
        }
    }

    /// Exactly what is persisted.
    pub fn stored_json(&self) -> String {
        serde_json::to_string_pretty(&self.snapshot).unwrap_or_default()
    }

    pub fn open_binding_for_tab(&self, scope: &ChatScope, tab_id: &str) -> Option<&ThreadBinding> {
        self.snapshot
            .bindings
            .iter()
            .find(|b| !b.closed && b.scope == *scope && b.tab_id == tab_id)
    }

    pub fn binding_for_thread(&self, scope: &ChatScope, thread_id: i64) -> Option<&ThreadBinding> {
        self.snapshot
            .bindings
            .iter()
            .find(|b| b.scope == *scope && b.thread_id == thread_id)
    }

    pub fn open_bindings_for_tab(&self, tab_id: &str) -> Vec<ThreadBinding> {
        self.snapshot
            .bindings
            .iter()
            .filter(|b| !b.closed && b.tab_id == tab_id)
            .cloned()
            .collect()
    }

    pub fn has_open_bindings(&self, scope: &ChatScope) -> bool {
        self.snapshot
            .bindings
            .iter()
            .any(|b| !b.closed && b.scope == *scope)
    }

    pub fn bind(&mut self, binding: ThreadBinding) {
        self.snapshot.bindings.retain(|existing| {
            let same_tab = !existing.closed
                && existing.scope == binding.scope
                && existing.tab_id == binding.tab_id;
            let same_thread =
                existing.scope == binding.scope && existing.thread_id == binding.thread_id;
            !same_tab && !same_thread
        });
        self.snapshot.bindings.push(binding);
        self.persist();
    }

    pub fn mark_intro_posted(&mut self, scope: &ChatScope, thread_id: i64) {
        if let Some(binding) = self
            .snapshot
            .bindings
            .iter_mut()
            .find(|b| b.scope == *scope && b.thread_id == thread_id)
        {
            binding.intro_posted = true;
            self.persist();
        }
    }

    pub fn set_stale(&mut self, scope: &ChatScope, thread_id: i64, stale: bool) {
        if let Some(binding) = self
            .snapshot
            .bindings
            .iter_mut()
            .find(|b| b.scope == *scope && b.thread_id == thread_id && b.stale != stale)
        {
            binding.stale = stale;
            self.persist();
        }
    }

    pub fn remove(&mut self, scope: &ChatScope, thread_id: i64) -> Option<ThreadBinding> {
        let index = self
            .snapshot
            .bindings
            .iter()
            .position(|b| b.scope == *scope && b.thread_id == thread_id)?;
        let removed = self.snapshot.bindings.remove(index);
        self.persist();
        Some(removed)
    }

    pub fn mark_closed(
        &mut self,
        scope: &ChatScope,
        thread_id: i64,
        topic_name: String,
    ) -> Option<ThreadBinding> {
        let binding = self
            .snapshot
            .bindings
            .iter_mut()
            .find(|b| b.scope == *scope && b.thread_id == thread_id)?;
        binding.closed = true;
        binding.topic_name = topic_name;
        let closed = binding.clone();
        let closed_count = self.snapshot.bindings.iter().filter(|b| b.closed).count();
        if closed_count > MAX_CLOSED_BINDINGS {
            if let Some(oldest) = self.snapshot.bindings.iter().position(|b| b.closed) {
                self.snapshot.bindings.remove(oldest);
            }
        }
        self.persist();
        Some(closed)
    }

    pub fn record_origin(&mut self, scope: &ChatScope, message_id: i64, tab_id: &str) {
        self.snapshot
            .origins
            .retain(|origin| !(origin.scope == *scope && origin.message_id == message_id));
        self.snapshot.origins.push(MessageOrigin {
            scope: scope.clone(),
            message_id,
            tab_id: tab_id.to_string(),
        });
        let overflow = self
            .snapshot
            .origins
            .len()
            .saturating_sub(MAX_MESSAGE_ORIGINS);
        self.snapshot.origins.drain(..overflow);
        self.persist();
    }

    pub fn origin_tab(&self, scope: &ChatScope, message_id: i64) -> Option<&str> {
        self.snapshot
            .origins
            .iter()
            .find(|origin| origin.scope == *scope && origin.message_id == message_id)
            .map(|origin| origin.tab_id.as_str())
    }

    pub fn topics_unavailable(&self, scope: &ChatScope) -> bool {
        self.topics_unavailable.contains(scope)
    }

    /// Returns whether the owner still needs the enable-topics notice; it is
    /// owed once per chat for the life of this store.
    pub fn mark_topics_unavailable(&mut self, scope: &ChatScope) -> bool {
        self.topics_unavailable.insert(scope.clone());
        if self.snapshot.topics_notice_sent.contains(scope) {
            return false;
        }
        self.snapshot.topics_notice_sent.push(scope.clone());
        self.persist();
        true
    }

    pub fn clear_topics_unavailable(&mut self) {
        self.topics_unavailable.clear();
    }

    fn persist(&self) {
        let Some(path) = self.path.as_deref() else {
            return;
        };
        let Ok(raw) = serde_json::to_string_pretty(&self.snapshot) else {
            return;
        };
        if let Err(error) = state_file(path).save(&raw) {
            log::warn!(
                "[bridge-threads] failed to persist {}: {error}",
                path.display()
            );
        }
    }
}

fn state_file(path: &Path) -> StateFile {
    StateFile::new(path).with_validator(validate_snapshot)
}

fn parse_snapshot(raw: &str) -> Result<Snapshot, String> {
    serde_json::from_str(raw).map_err(|error| error.to_string())
}

fn validate_snapshot(raw: &str) -> Result<(), String> {
    parse_snapshot(raw).map(|_| ())
}

#[cfg(test)]
mod persistence_tests {
    use super::*;

    fn scratch_path() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("bridge-threads-{}", uuid_like()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("telegram-threads.json")
    }

    fn uuid_like() -> String {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        format!("{}-{nanos}", std::process::id())
    }

    fn binding() -> ThreadBinding {
        ThreadBinding {
            scope: ChatScope {
                service: "telegram".into(),
                bot_id: 1,
                chat_id: 2,
            },
            tab_id: "tab".into(),
            thread_id: 3,
            topic_name: "Tab".into(),
            intro_posted: false,
            closed: false,
            stale: false,
        }
    }

    #[test]
    fn corrupt_registry_recovers_bindings_and_never_saves_defaults_over_it() {
        let path = scratch_path();
        ThreadRegistry::load(path.clone()).bind(binding());
        std::fs::write(&path, "{\"bindings\": [").unwrap();

        let recovered = ThreadRegistry::load(path.clone());
        assert!(recovered.binding_for_thread(&binding().scope, 3).is_some());

        std::fs::remove_dir_all(state_file(&path).backup_dir()).unwrap();
        std::fs::write(&path, "{\"bindings\": [").unwrap();
        let mut held = ThreadRegistry::load(path.clone());
        held.bind(binding());
        assert!(!path.exists());
        let copies = state_file(&path).corrupt_copies();
        assert_eq!(
            std::fs::read_to_string(copies.last().unwrap()).unwrap(),
            "{\"bindings\": ["
        );
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
