use crate::agent::provider::{SessionContentQuery, SessionContentSource};
use crate::multiplexer::types::Tab;
use crate::service::ServiceContext;
use crate::session::lifecycle::{
    JournalLoad, JournalRecord, LifecycleTransition, TabLifecycleJournal,
};

#[derive(Debug, PartialEq, Eq)]
enum MissingSessionAction {
    Fresh,
    ConversationMissing,
    EvidenceUnknown,
}

fn classify_missing_session(
    session_exists: Option<bool>,
    journal_conversation: Option<bool>,
    provider_conversation: Option<bool>,
    launch_captured: bool,
) -> MissingSessionAction {
    if journal_conversation == Some(true) || provider_conversation == Some(true) {
        MissingSessionAction::ConversationMissing
    } else if session_exists == Some(false)
        && journal_conversation == Some(false)
        && provider_conversation == Some(false)
        && launch_captured
    {
        MissingSessionAction::Fresh
    } else {
        MissingSessionAction::EvidenceUnknown
    }
}

fn journal_conversation(load: &JournalLoad, tab_id: &str, session_id: &str) -> Option<bool> {
    let conversation = load.records.iter().any(|record| matches!(record,
        JournalRecord::Transition { entry }
            if entry.key.tab_id == tab_id && matches!(&entry.transition,
                LifecycleTransition::ConversationStarted { session_id: recorded } if recorded == session_id)
    ));
    if conversation {
        return Some(true);
    }
    (load.tail.is_intact() && load.duplicate_seqs == 0).then_some(false)
}

fn quarantined_conversation(
    path: &std::path::Path,
    tab_id: &str,
    session_id: &str,
) -> Option<bool> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(false),
        Err(_) => return None,
    };
    if !metadata.is_file() || metadata.len() > crate::agent::registry::DEFAULT_SESSION_FILE_BYTE_CAP
    {
        return None;
    }
    let body = std::fs::read_to_string(path).ok()?;
    let mut load = JournalLoad::empty();
    let mut malformed = false;
    for line in body.lines().filter(|line| !line.trim().is_empty()) {
        match serde_json::from_str::<JournalRecord>(line) {
            Ok(record) => load.records.push(record),
            Err(_) => malformed = true,
        }
    }
    let evidence = journal_conversation(&load, tab_id, session_id);
    if evidence == Some(true) {
        evidence
    } else {
        (!malformed).then_some(false)
    }
}

fn provider_conversation(
    service: &ServiceContext,
    provider_id: &str,
    session_id: &str,
    cwd: &str,
) -> Option<bool> {
    service
        .agent_registry
        .with_provider(provider_id, |provider| {
            provider_conversation_content(provider, provider_id, session_id, cwd)
        })
        .flatten()
}

fn provider_conversation_content(
    provider: &dyn crate::agent::provider::AgentProvider,
    provider_id: &str,
    session_id: &str,
    cwd: &str,
) -> Option<bool> {
    let meta = provider.find_session(session_id);
    if meta
        .as_ref()
        .is_some_and(|meta| meta.provider != provider_id || meta.session_id != session_id)
    {
        return None;
    }
    match provider.session_content_source() {
        SessionContentSource::None => None,
        SessionContentSource::SharedStore => {
            let content = provider.read_session_content(
                session_id,
                &SessionContentQuery {
                    max_bytes: crate::agent::registry::DEFAULT_SESSION_FILE_BYTE_CAP,
                    ..Default::default()
                },
            )?;
            if content
                .messages
                .iter()
                .any(|message| message.role == "user" && !message.is_meta)
            {
                Some(true)
            } else {
                (!content.truncated).then_some(false)
            }
        }
        SessionContentSource::SessionFile => {
            let path = meta
                .as_ref()
                .map(|meta| {
                    std::path::PathBuf::from(
                        meta.transcript_path.as_deref().unwrap_or(&meta.file_path),
                    )
                })
                .or_else(|| provider.session_file_path(cwd, session_id))?;
            let path = path.as_path();
            file_conversation(provider, path)
        }
    }
}

fn file_conversation(
    provider: &dyn crate::agent::provider::AgentProvider,
    path: &std::path::Path,
) -> Option<bool> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(false),
        Err(_) => return None,
    };
    if !metadata.is_file()
        || metadata.len() > crate::agent::registry::DEFAULT_SESSION_FILE_BYTE_CAP
        || std::fs::File::open(path).is_err()
    {
        return None;
    }
    let messages = provider.parse_session_messages(path);
    if messages
        .iter()
        .any(|message| message.role == "user" && !message.is_meta)
    {
        return Some(true);
    }
    (metadata.len() == 0).then_some(false)
}

fn clear_empty_session(tab: &mut Tab) {
    tab.remember_superseded_binding_nonce();
    tab.agent_session_id = None;
    tab.agent_session_name = None;
    tab.session_binding_nonce = None;
    tab.reported_session_binding_nonce = None;
    if let Some(restore) = tab.close_restore.as_mut() {
        restore.exact_session = false;
    }
}

fn unresolved_user_turn(
    states: impl IntoIterator<
        Item = crate::service::pending_delivery_payloads::user_sends::UserSendState,
    >,
) -> bool {
    states.into_iter().any(|state| {
        state != crate::service::pending_delivery_payloads::user_sends::UserSendState::Queued
    })
}

impl ServiceContext {
    pub(crate) fn record_provider_conversation(&self, tab_id: &str) {
        let identity = {
            let mux = self.mux.read();
            mux.get_tab(tab_id).and_then(|tab| {
                Some((
                    tab.agent_type.clone()?,
                    tab.agent_session_id.clone()?,
                    tab.cwd.clone()?,
                ))
            })
        };
        let Some((provider, session, cwd)) = identity else {
            return;
        };
        if provider_conversation(self, &provider, &session, &cwd) == Some(true) {
            self.note_session_conversation(tab_id, &session);
        }
    }

    pub(super) fn prepare_empty_session_restore(
        &self,
        tab_id: &str,
        provider_id: &str,
        session_id: Option<&str>,
        session_exists: Option<bool>,
    ) -> bool {
        self.prepare_empty_session_restore_in_journal(
            tab_id,
            provider_id,
            session_id,
            session_exists,
            &self.lifecycle_journal,
        )
    }

    fn prepare_empty_session_restore_in_journal(
        &self,
        tab_id: &str,
        provider_id: &str,
        session_id: Option<&str>,
        session_exists: Option<bool>,
        journal_store: &TabLifecycleJournal,
    ) -> bool {
        let Some(session_id) = session_id else {
            return false;
        };
        if session_exists != Some(false) {
            return false;
        }
        let Some((captured, cwd)) = self.mux.read().get_tab(tab_id).map(|tab| {
            (
                tab.restorable_launch.captured,
                tab.close_restore
                    .as_ref()
                    .and_then(|restore| restore.session_cwd.clone())
                    .or_else(|| tab.cwd.clone()),
            )
        }) else {
            return false;
        };
        let Some(cwd) = cwd else {
            return false;
        };
        let journal = journal_store.load();
        let journal_evidence = journal_conversation(&journal, tab_id, session_id);
        let journal_evidence = if journal_evidence != Some(true) {
            match quarantined_conversation(
                &crate::session::lifecycle::refused_tail_path(journal_store.path()),
                tab_id,
                session_id,
            ) {
                Some(false) => journal_evidence,
                evidence => evidence,
            }
        } else {
            journal_evidence
        };
        let ledger = self
            .chat_send_ledger
            .session_conversation(tab_id, session_id);
        if ledger != Some(false) {
            return false;
        }
        if classify_missing_session(
            session_exists,
            journal_evidence,
            provider_conversation(self, provider_id, session_id, &cwd),
            captured,
        ) != MissingSessionAction::Fresh
        {
            return false;
        }
        if self
            .pending_delivery_payloads
            .user_sends
            .records(tab_id)
            .map_or(true, |records| {
                unresolved_user_turn(records.iter().map(|record| record.state))
            })
        {
            return false;
        }
        let key = { self.mux.read().lifecycle_key(tab_id) };
        let Some(key) = key.or_else(|| self.open_lifecycle_epoch(tab_id)) else {
            return false;
        };
        {
            let mut mux = self.mux.write();
            let Some(tab) = mux.get_tab_mut(tab_id) else {
                return false;
            };
            if tab.agent_type.as_deref() != Some(provider_id)
                || tab.agent_session_id.as_deref() != Some(session_id)
                || tab.agent_booting
                || !tab.restored_from_snapshot
                || tab.lifecycle_epoch != Some(key.epoch.0)
                || tab.lifecycle_operation_id.as_deref() != Some(key.operation_id.as_str())
            {
                return false;
            }
            clear_empty_session(tab);
        }
        crate::service::tab_lifecycle::append(journal_store, key.clone(), LifecycleTransition::RolledBack {
            reason: format!("empty unavailable session {session_id}; restoring fresh with recorded settings"),
        });
        crate::service::tab_lifecycle::append(
            journal_store,
            key,
            LifecycleTransition::EmptySessionReplaced {
                session_id: session_id.into(),
            },
        );
        self.schedule_session_persist_debounced();
        true
    }

    pub(super) fn emit_empty_restore_notice(&self, tab_id: &str) {
        let mut notices = Vec::new();
        crate::service::tab_chat::restore_notice::append_restore_notice(
            &self.lifecycle_journal,
            tab_id,
            &mut notices,
        );
        let authority = self.mux.read().get_tab(tab_id).map(|tab| {
            (
                tab.agent_session_id.clone(),
                tab.session_binding_nonce.clone(),
            )
        });
        let Some((session, nonce)) = authority else {
            return;
        };
        crate::service::tab_chat::structured_sink::emit_chat_msgs(
            self.ui.as_ref(),
            tab_id,
            &notices,
            None,
            session.as_deref(),
            nonce.as_deref(),
        );
    }
}

#[cfg(test)]
mod tests;
