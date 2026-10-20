use super::{JournalRecord, LifecycleTransition};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Default)]
pub(super) struct JournalReadIndex {
    restored: HashMap<String, String>,
    conversations: HashMap<String, HashSet<String>>,
}

enum Change<'a> {
    Restored(&'a str, &'a str),
    Conversation(&'a str, &'a str),
}

impl<'a> Change<'a> {
    fn from_record(record: &'a JournalRecord) -> Option<Self> {
        let JournalRecord::Transition { entry } = record else {
            return None;
        };
        match &entry.transition {
            LifecycleTransition::EmptySessionReplaced { session_id } => {
                Some(Self::Restored(&entry.key.tab_id, session_id))
            }
            LifecycleTransition::ConversationStarted { session_id } => {
                Some(Self::Conversation(&entry.key.tab_id, session_id))
            }
            _ => None,
        }
    }
}

impl JournalReadIndex {
    pub(super) fn from_records(records: &[JournalRecord]) -> Self {
        let mut index = Self::default();
        for change in records.iter().filter_map(Change::from_record) {
            index.apply(change);
        }
        index
    }

    pub(super) fn updated(&self, record: &JournalRecord) -> Option<Self> {
        let change = Change::from_record(record)?;
        let mut next = self.clone();
        next.apply(change);
        Some(next)
    }

    fn apply(&mut self, change: Change<'_>) {
        match change {
            Change::Restored(tab, session) => {
                self.restored.insert(tab.into(), session.into());
            }
            Change::Conversation(tab, session) => {
                self.conversations
                    .entry(tab.into())
                    .or_default()
                    .insert(session.into());
            }
        }
    }

    pub(super) fn restored_session(&self, tab: &str) -> Option<String> {
        self.restored.get(tab).cloned()
    }

    pub(super) fn has_conversation(&self, tab: &str, session: &str) -> bool {
        self.conversations
            .get(tab)
            .is_some_and(|sessions| sessions.contains(session))
    }
}

#[cfg(test)]
mod tests;
