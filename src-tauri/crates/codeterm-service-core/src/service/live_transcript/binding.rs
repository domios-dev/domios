use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::{fs, io};

use serde::{Deserialize, Serialize};

use super::store::{
    encoded_record_path, invalid_data, invalid_input, read_private, refused, validate_record_id,
    validate_tab_id, write_private, StoreState, MAX_RECORD_BYTES,
};
use super::{
    BindingTarget, ClosedTranscriptOwner, CursorStart, EpochHistory, RoutedAction,
    RoutedActionRecord, RoutedActionsPage, TranscriptBinding, TranscriptCursor,
    TranscriptOwnerState, TranscriptSeq, TranscriptStore,
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct BindingDocument {
    pub(super) binding: TranscriptBinding,
    pub(super) owner: TranscriptOwnerState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) reopened_from: Option<ClosedTranscriptOwner>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    suspended_epoch_id: Option<String>,
    initial_cursor_seq: TranscriptSeq,
}

impl BindingDocument {
    pub(super) fn load(
        root: &Path,
        owner_tab_id: &str,
        committed_seq: TranscriptSeq,
    ) -> io::Result<Self> {
        let path = root.join("binding.json");
        let binding = if let Some(document) = read_private::<Self>(&path, MAX_RECORD_BYTES)? {
            document
        } else {
            let document = Self {
                binding: TranscriptBinding {
                    owner_tab_id: owner_tab_id.to_owned(),
                    revision: 0,
                    target: BindingTarget::Detached,
                },
                owner: TranscriptOwnerState::Open {
                    live_tab_id: owner_tab_id.to_owned(),
                },
                reopened_from: None,
                suspended_epoch_id: None,
                initial_cursor_seq: 0,
            };
            write_private(&path, &document, MAX_RECORD_BYTES)?;
            document
        };
        if binding.binding.owner_tab_id != owner_tab_id
            || binding.initial_cursor_seq > committed_seq
        {
            return Err(invalid_data(
                "Transcript binding identity or frontier mismatch",
            ));
        }
        match &binding.owner {
            TranscriptOwnerState::Open { live_tab_id } => validate_tab_id(live_tab_id)?,
            TranscriptOwnerState::Closed { identity }
            | TranscriptOwnerState::Retired { identity } => {
                super::close_retention::validate_closed_identity(identity)?;
            }
        }
        if let Some(identity) = &binding.reopened_from {
            super::close_retention::validate_closed_identity(identity)?;
        }
        if let Some(epoch_id) = &binding.suspended_epoch_id {
            let epoch = super::epoch_index::require_epoch(root, owner_tab_id, epoch_id)?;
            if binding.binding.target != BindingTarget::Detached
                || epoch.activation_revision >= binding.binding.revision
            {
                return Err(invalid_data("Invalid suspended transcript epoch"));
            }
        }
        match &binding.binding.target {
            BindingTarget::Attached { epoch_id } => {
                let epoch = super::epoch_index::require_epoch(root, owner_tab_id, epoch_id)?;
                if epoch.activation_revision >= binding.binding.revision
                    || (matches!(binding.owner, TranscriptOwnerState::Open { .. })
                        && epoch.history.archive.is_some())
                {
                    return Err(invalid_data("Invalid attached transcript epoch"));
                }
            }
            BindingTarget::External { tab_id } => validate_tab_id(tab_id)?,
            BindingTarget::Detached if binding.initial_cursor_seq != 0 => {
                return Err(invalid_data("Detached transcript has an initial cursor"));
            }
            BindingTarget::Detached => {}
        }
        binding.active_cursor(root, committed_seq)?;
        Ok(binding)
    }

    pub(super) fn require_live_owner(&self, tab_id: &str) -> io::Result<()> {
        match &self.owner {
            TranscriptOwnerState::Open { live_tab_id } if live_tab_id == tab_id => Ok(()),
            _ => Err(refused("Transcript live owner identity mismatch")),
        }
    }

    pub(super) fn require_open(&self) -> io::Result<()> {
        match self.owner {
            TranscriptOwnerState::Open { .. } => Ok(()),
            _ => Err(refused("Transcript owner is closed")),
        }
    }

    pub(super) fn require_revision(&self, expected_revision: u64) -> io::Result<()> {
        if self.binding.revision != expected_revision {
            return Err(refused("Stale transcript binding revision"));
        }
        Ok(())
    }

    pub(super) fn next_revision(&mut self) -> io::Result<()> {
        self.binding.revision = self
            .binding
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid_data("Transcript binding revision exhausted"))?;
        Ok(())
    }

    pub(super) fn detach(&mut self) {
        if let BindingTarget::Attached { epoch_id } = &self.binding.target {
            self.suspended_epoch_id = Some(epoch_id.clone());
        }
        self.binding.target = BindingTarget::Detached;
        self.initial_cursor_seq = 0;
    }

    pub(super) fn active_cursor(
        &self,
        root: &Path,
        committed_seq: TranscriptSeq,
    ) -> io::Result<Option<TranscriptCursor>> {
        let Some(key) = self.consumer_key() else {
            return Ok(None);
        };
        Ok(Some(
            read_cursor(root, &key, committed_seq)?
                .unwrap_or_else(|| initial_cursor(key, self.initial_cursor_seq)),
        ))
    }

    fn consumer_key(&self) -> Option<String> {
        consumer_key(&self.binding.owner_tab_id, &self.binding.target)
    }

    pub(super) fn persist_active_cursor(
        &self,
        root: &Path,
        committed_seq: TranscriptSeq,
    ) -> io::Result<()> {
        if let Some(key) = self.consumer_key() {
            if read_cursor(root, &key, committed_seq)?.is_none() {
                save_cursor(root, &initial_cursor(key, self.initial_cursor_seq))?;
            }
        }
        Ok(())
    }
}

impl TranscriptStore {
    pub fn binding(&self) -> TranscriptBinding {
        self.inner.state.lock().binding.binding.clone()
    }

    pub(in crate::service) fn verify_live_binding(
        &self,
        owner: &str,
        binding: &TranscriptBinding,
    ) -> io::Result<()> {
        let state = self.inner.state.lock();
        state.require_healthy()?;
        state.binding.require_live_owner(owner)?;
        if &state.binding.binding != binding {
            return Err(refused("Stale or foreign transcript binding"));
        }
        Ok(())
    }

    pub fn suspended_epoch(&self) -> io::Result<Option<EpochHistory>> {
        let state = self.inner.state.lock();
        state.require_healthy()?;
        state
            .binding
            .suspended_epoch_id
            .as_deref()
            .map(|epoch_id| {
                super::epoch_index::require_epoch(
                    &self.inner.root,
                    &state.binding.binding.owner_tab_id,
                    epoch_id,
                )
                .map(|document| document.history)
            })
            .transpose()
    }

    pub fn suspend_attached_epoch(
        &self,
        expected: &TranscriptBinding,
    ) -> io::Result<TranscriptBinding> {
        let mut state = self.inner.state.lock();
        self.require_cursor(&state, expected)?;
        let BindingTarget::Attached { .. } = &expected.target else {
            return Err(refused("Transcript has no attached router to suspend"));
        };
        let mut document = state.binding.clone();
        document.next_revision()?;
        document.detach();
        state
            .binding
            .persist_active_cursor(&self.inner.root, state.last_seq())?;
        let binding = document.binding.clone();
        self.save_binding(&mut state, document)?;
        Ok(binding)
    }

    pub fn set_binding(
        &self,
        expected_revision: u64,
        target: BindingTarget,
        start: CursorStart,
    ) -> io::Result<TranscriptBinding> {
        let mut state = self.inner.state.lock();
        state.require_healthy()?;
        state.binding.require_open()?;
        state.binding.require_revision(expected_revision)?;
        if state.binding.binding.target == target {
            return Ok(state.binding.binding.clone());
        }
        if let BindingTarget::Attached { epoch_id } = &state.binding.binding.target {
            let epoch = super::epoch_index::require_epoch(
                &self.inner.root,
                &state.binding.binding.owner_tab_id,
                epoch_id,
            )?;
            if epoch.archive_source.is_some() {
                return Err(refused(
                    "Suspend the exact transcript router before changing its binding",
                ));
            }
        }
        if let Some(epoch_id) = &state.binding.suspended_epoch_id {
            let epoch = super::epoch_index::require_epoch(
                &self.inner.root,
                &state.binding.binding.owner_tab_id,
                epoch_id,
            )?;
            if epoch.archive_source.is_some() && epoch.history.archive.is_none() {
                return Err(refused("Suspended transcript epoch has no archive"));
            }
        }
        match &target {
            BindingTarget::Attached { epoch_id } => {
                let epoch = super::epoch_index::require_epoch(
                    &self.inner.root,
                    &state.binding.binding.owner_tab_id,
                    epoch_id,
                )?;
                if epoch.activation_revision != expected_revision || epoch.history.archive.is_some()
                {
                    return Err(refused("Transcript epoch is ineligible for activation"));
                }
            }
            BindingTarget::External { tab_id } => validate_tab_id(tab_id)?,
            BindingTarget::Detached => {}
        }

        let committed_seq = state.last_seq();
        let mut document = state.binding.clone();
        document.next_revision()?;
        document.initial_cursor_seq = match consumer_key(&document.binding.owner_tab_id, &target) {
            Some(key) => {
                if let Some(cursor) = read_cursor(&self.inner.root, &key, committed_seq)? {
                    cursor.acked_seq
                } else if state.binding.consumer_key().as_deref() == Some(&key) {
                    state.binding.initial_cursor_seq
                } else {
                    match (&target, start) {
                        (BindingTarget::Attached { .. }, _) | (_, CursorStart::Beginning) => 0,
                        (_, CursorStart::Latest) => committed_seq,
                        (_, CursorStart::After(seq)) if seq <= committed_seq => seq,
                        (_, CursorStart::After(_)) => {
                            return Err(invalid_input("Cursor start exceeds committed history"))
                        }
                    }
                }
            }
            None => 0,
        };
        document.binding.target = target;
        document.suspended_epoch_id = None;
        state
            .binding
            .persist_active_cursor(&self.inner.root, committed_seq)?;
        let binding = document.binding.clone();
        self.save_binding(&mut state, document)?;
        Ok(binding)
    }

    pub fn cursor(&self, binding: &TranscriptBinding) -> io::Result<TranscriptCursor> {
        let state = self.inner.state.lock();
        self.require_cursor(&state, binding)
    }

    /// The built-in cursor is available before a replacement epoch is activated.
    pub fn attached_cursor(&self) -> io::Result<TranscriptCursor> {
        let state = self.inner.state.lock();
        state.require_healthy()?;
        let key = format!("concierge:{}", state.binding.binding.owner_tab_id);
        Ok(read_cursor(&self.inner.root, &key, state.last_seq())?
            .unwrap_or_else(|| initial_cursor(key, 0)))
    }

    pub fn ack(
        &self,
        binding: &TranscriptBinding,
        through_seq: TranscriptSeq,
    ) -> io::Result<TranscriptCursor> {
        self.advance_cursor(binding, through_seq, true)
    }

    pub fn mark_seen(
        &self,
        binding: &TranscriptBinding,
        through_seq: TranscriptSeq,
    ) -> io::Result<TranscriptCursor> {
        self.advance_cursor(binding, through_seq, false)
    }

    fn advance_cursor(
        &self,
        binding: &TranscriptBinding,
        through_seq: TranscriptSeq,
        ack: bool,
    ) -> io::Result<TranscriptCursor> {
        let mut state = self.inner.state.lock();
        let previous = self.require_cursor(&state, binding)?;
        if through_seq > state.last_seq() {
            return Err(invalid_input("Cursor cannot exceed committed history"));
        }
        let mut cursor = previous.clone();
        if ack {
            cursor.acked_seq = cursor.acked_seq.max(through_seq);
        }
        cursor.seen_seq = cursor.seen_seq.max(through_seq);
        if cursor != previous {
            if let Err(error) = save_cursor(&self.inner.root, &cursor) {
                state.fence_writes();
                return Err(error);
            }
        }
        Ok(cursor)
    }

    pub fn record_action(
        &self,
        binding: &TranscriptBinding,
        action: RoutedAction,
    ) -> io::Result<RoutedActionRecord> {
        let mut state = self.inner.state.lock();
        let cursor = self.require_cursor(&state, binding)?;
        validate_action(&action, state.last_seq())?;
        if let Some(previous) = self.read_action_locked(&state, &action.action_id)? {
            return if previous.binding == *binding && previous.action == action {
                Ok(previous)
            } else {
                Err(refused("Routed transcript action ID is already committed"))
            };
        }
        let action_seq = state.action_ids.len() as u64 + 1;
        let path = action_path(&self.inner.root, &action.action_id);
        let record = RoutedActionRecord {
            action_seq,
            binding: binding.clone(),
            consumer_key: cursor.consumer_key,
            action,
        };
        if let Err(error) = write_private(&path, &record, MAX_RECORD_BYTES) {
            state.fence_writes();
            return Err(error);
        }
        state.action_ids.push(record.action.action_id.clone());
        Ok(record)
    }

    pub fn routed_action(&self, action_id: &str) -> io::Result<Option<RoutedActionRecord>> {
        let state = self.inner.state.lock();
        state.require_healthy()?;
        self.read_action_locked(&state, action_id)
    }

    pub fn routed_actions(&self, after_seq: u64, limit: usize) -> io::Result<RoutedActionsPage> {
        let state = self.inner.state.lock();
        state.require_healthy()?;
        let start = usize::try_from(after_seq)
            .ok()
            .filter(|start| *start <= state.action_ids.len())
            .ok_or_else(|| invalid_input("Routed transcript action position exceeds history"))?;
        if !(1..=100).contains(&limit) {
            return Err(invalid_input("Invalid routed transcript action limit"));
        }
        let end = (start + limit).min(state.action_ids.len());
        let actions = state.action_ids[start..end]
            .iter()
            .map(|id| {
                self.read_action_locked(&state, id)?
                    .ok_or_else(|| invalid_data("Routed transcript action is missing"))
            })
            .collect::<io::Result<Vec<_>>>()?;
        let next_after_seq = (end < state.action_ids.len()).then_some(end as u64);
        Ok(RoutedActionsPage {
            actions,
            next_after_seq,
        })
    }

    fn read_action_locked(
        &self,
        state: &StoreState,
        action_id: &str,
    ) -> io::Result<Option<RoutedActionRecord>> {
        validate_record_id(action_id)?;
        let record = read_private::<RoutedActionRecord>(
            &action_path(&self.inner.root, action_id),
            MAX_RECORD_BYTES,
        )?;
        if let Some(record) = &record {
            validate_action_record(
                &self.inner.root,
                record,
                &state.binding.binding,
                state.last_seq(),
            )?;
            let index = usize::try_from(record.action_seq - 1).ok();
            if record.action.action_id != action_id
                || index
                    .and_then(|index| state.action_ids.get(index))
                    .map(String::as_str)
                    != Some(action_id)
            {
                return Err(invalid_data("Routed transcript action index mismatch"));
            }
        }
        Ok(record)
    }

    pub(super) fn require_cursor(
        &self,
        state: &StoreState,
        binding: &TranscriptBinding,
    ) -> io::Result<TranscriptCursor> {
        state.require_healthy()?;
        state.binding.require_open()?;
        if &state.binding.binding != binding {
            return Err(refused("Stale or foreign transcript binding"));
        }
        state
            .binding
            .active_cursor(&self.inner.root, state.last_seq())?
            .ok_or_else(|| refused("Transcript has no active consumer"))
    }

    pub(super) fn save_binding(
        &self,
        state: &mut StoreState,
        document: BindingDocument,
    ) -> io::Result<()> {
        if let Err(error) = write_private(
            &self.inner.root.join("binding.json"),
            &document,
            MAX_RECORD_BYTES,
        ) {
            state.fence_writes();
            return Err(error);
        }
        state.binding = document;
        Ok(())
    }
}

pub(super) fn recover_actions(
    root: &Path,
    binding: &TranscriptBinding,
    committed_seq: TranscriptSeq,
) -> io::Result<Vec<String>> {
    let mut actions = BTreeMap::new();
    for entry in fs::read_dir(root.join("route_actions"))? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or_else(|| invalid_data("Non-UTF-8 transcript action name"))?;
        if name.starts_with('.') && name.ends_with(".tmp") {
            continue;
        }
        let record = read_private::<RoutedActionRecord>(&entry.path(), MAX_RECORD_BYTES)?
            .ok_or_else(|| invalid_data("Routed transcript action is missing"))?;
        validate_action_record(root, &record, binding, committed_seq)?;
        if action_path(root, &record.action.action_id)
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            != Some(name)
            || actions
                .insert(record.action_seq, record.action.action_id)
                .is_some()
        {
            return Err(invalid_data("Routed transcript action index mismatch"));
        }
    }
    actions
        .into_iter()
        .enumerate()
        .map(|(index, (seq, id))| {
            if seq != index as u64 + 1 {
                return Err(invalid_data("Gap in routed transcript action sequences"));
            }
            Ok(id)
        })
        .collect()
}

fn action_path(root: &Path, id: &str) -> PathBuf {
    encoded_record_path(&root.join("route_actions"), id)
}

fn validate_action_record(
    root: &Path,
    record: &RoutedActionRecord,
    binding: &TranscriptBinding,
    committed_seq: TranscriptSeq,
) -> io::Result<()> {
    if record.action_seq == 0
        || record.binding.owner_tab_id != binding.owner_tab_id
        || record.binding.revision == 0
        || record.binding.revision > binding.revision
        || consumer_key(&record.binding.owner_tab_id, &record.binding.target).as_deref()
            != Some(record.consumer_key.as_str())
    {
        return Err(invalid_data("Routed transcript action identity mismatch"));
    }
    match &record.binding.target {
        BindingTarget::Attached { epoch_id } => {
            let epoch = super::epoch_index::require_epoch(root, &binding.owner_tab_id, epoch_id)?;
            if epoch.activation_revision.checked_add(1) != Some(record.binding.revision) {
                return Err(invalid_data("Routed transcript action epoch mismatch"));
            }
        }
        BindingTarget::External { tab_id } => validate_tab_id(tab_id)?,
        BindingTarget::Detached => {
            return Err(invalid_data("Routed transcript action has no consumer"))
        }
    }
    validate_action(&record.action, committed_seq)
}

fn validate_action(action: &RoutedAction, committed_seq: TranscriptSeq) -> io::Result<()> {
    validate_record_id(&action.action_id)?;
    if action.range.from_seq == 0
        || action.range.from_seq > action.range.to_seq
        || action.range.to_seq > committed_seq
        || action.destination.is_empty()
        || action.destination.len() > 512
        || action.rationale.len() > 4096
    {
        return Err(invalid_input("Invalid routed transcript action"));
    }
    Ok(())
}

fn consumer_key(owner_tab_id: &str, target: &BindingTarget) -> Option<String> {
    match target {
        BindingTarget::Attached { .. } => Some(format!("concierge:{owner_tab_id}")),
        BindingTarget::External { tab_id } => Some(format!("tab:{tab_id}")),
        BindingTarget::Detached => None,
    }
}

fn cursor_path(root: &Path, key: &str) -> PathBuf {
    encoded_record_path(&root.join("cursors"), key)
}

fn read_cursor(
    root: &Path,
    key: &str,
    committed_seq: TranscriptSeq,
) -> io::Result<Option<TranscriptCursor>> {
    let cursor = read_private::<TranscriptCursor>(&cursor_path(root, key), MAX_RECORD_BYTES)?;
    if cursor.as_ref().is_some_and(|cursor| {
        cursor.consumer_key != key
            || cursor.acked_seq > cursor.seen_seq
            || cursor.seen_seq > committed_seq
    }) {
        return Err(invalid_data(
            "Transcript cursor identity or frontier mismatch",
        ));
    }
    Ok(cursor)
}

fn save_cursor(root: &Path, cursor: &TranscriptCursor) -> io::Result<()> {
    write_private(
        &cursor_path(root, &cursor.consumer_key),
        cursor,
        MAX_RECORD_BYTES,
    )
}

fn initial_cursor(key: String, seq: TranscriptSeq) -> TranscriptCursor {
    TranscriptCursor {
        consumer_key: key,
        acked_seq: seq,
        seen_seq: seq,
    }
}
