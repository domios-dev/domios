use std::io;

use super::store::{
    active_index, invalid_data, invalid_input, refused, validate_record_id, validate_tab_id,
};
use super::{ClosedTranscriptOwner, TranscriptOwnerState, TranscriptRange, TranscriptStore};

pub struct TranscriptReader {
    store: TranscriptStore,
}

impl TranscriptReader {
    pub fn store(&self) -> &TranscriptStore {
        &self.store
    }
}

impl Drop for TranscriptReader {
    fn drop(&mut self) {
        self.store.inner.state.lock().readers -= 1;
    }
}

impl TranscriptStore {
    pub fn owner_state(&self) -> TranscriptOwnerState {
        self.inner.state.lock().binding.owner.clone()
    }

    /// The caller holds the host's verified owner or export authorization.
    pub fn pin_reader(&self) -> io::Result<TranscriptReader> {
        let mut state = self.inner.state.lock();
        state.require_healthy()?;
        state.readers = state
            .readers
            .checked_add(1)
            .ok_or_else(|| invalid_data("Transcript reader count exhausted"))?;
        Ok(TranscriptReader {
            store: self.clone(),
        })
    }

    /// Pass the exact history row only after the host has committed the owner close.
    pub fn close_owner(
        &self,
        live_tab_id: &str,
        identity: ClosedTranscriptOwner,
    ) -> io::Result<Option<TranscriptRange>> {
        validate_closed_identity(&identity)?;
        if identity.tab_id != live_tab_id {
            return Err(refused(
                "Closed history does not match the live transcript owner",
            ));
        }
        let mut state = self.inner.state.lock();
        state.require_healthy()?;
        if matches!(&state.binding.owner, TranscriptOwnerState::Closed { identity: closed } if closed == &identity)
        {
            return Ok(None);
        }
        state.binding.require_live_owner(live_tab_id)?;
        if state.binding.reopened_from.as_ref() == Some(&identity) {
            return Err(refused("Closed history record already reopened"));
        }
        let sealed = active_index(&state)
            .map(|index| self.seal_locked(&mut state, index))
            .transpose()?;
        let mut document = state.binding.clone();
        document.next_revision()?;
        document.owner = TranscriptOwnerState::Closed { identity };
        self.save_binding(&mut state, document)?;
        Ok(sealed)
    }

    /// The host has authorized reopening this same closed-history record.
    pub fn reopen_owner(
        &self,
        identity: &ClosedTranscriptOwner,
        live_tab_id: &str,
    ) -> io::Result<()> {
        validate_closed_identity(identity)?;
        validate_tab_id(live_tab_id)?;
        let mut state = self.inner.state.lock();
        state.require_healthy()?;
        if state.binding.require_live_owner(live_tab_id).is_ok()
            && state.binding.reopened_from.as_ref() == Some(identity)
        {
            return Ok(());
        }
        require_closed_owner(&state.binding.owner, identity)?;
        let mut document = state.binding.clone();
        document.next_revision()?;
        document.detach();
        document.owner = TranscriptOwnerState::Open {
            live_tab_id: live_tab_id.to_owned(),
        };
        document.reopened_from = Some(identity.clone());
        state
            .binding
            .persist_active_cursor(&self.inner.root, state.last_seq())?;
        self.save_binding(&mut state, document)
    }

    /// The host has established permanent retirement of this same closed-history record.
    pub fn retire_owner(&self, identity: &ClosedTranscriptOwner) -> io::Result<()> {
        validate_closed_identity(identity)?;
        let mut state = self.inner.state.lock();
        state.require_healthy()?;
        if matches!(&state.binding.owner, TranscriptOwnerState::Retired { identity: retired } if retired == identity)
        {
            return Ok(());
        }
        require_closed_owner(&state.binding.owner, identity)?;
        let mut document = state.binding.clone();
        document.next_revision()?;
        document.owner = TranscriptOwnerState::Retired {
            identity: identity.clone(),
        };
        self.save_binding(&mut state, document)
    }

    pub fn collect_retired(
        &self,
        identity: &ClosedTranscriptOwner,
    ) -> io::Result<Vec<TranscriptRange>> {
        validate_closed_identity(identity)?;
        let mut state = self.inner.state.lock();
        state.require_healthy()?;
        if !matches!(&state.binding.owner, TranscriptOwnerState::Retired { identity: retired } if retired == identity)
        {
            return Err(refused(
                "Transcript owner is still open or eligible for reopen",
            ));
        }
        if state.readers != 0 {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "Transcript has an active reader",
            ));
        }
        self.evict_locked(&mut state, 0)
    }
}

pub(super) fn validate_closed_identity(identity: &ClosedTranscriptOwner) -> io::Result<()> {
    validate_tab_id(&identity.tab_id)?;
    validate_record_id(&identity.group_id)?;
    if identity.closed_at_ms == 0 {
        return Err(invalid_input("Missing closed history timestamp"));
    }
    Ok(())
}

fn require_closed_owner(
    owner: &TranscriptOwnerState,
    identity: &ClosedTranscriptOwner,
) -> io::Result<()> {
    if !matches!(owner, TranscriptOwnerState::Closed { identity: closed } if closed == identity) {
        return Err(refused("Exact closed transcript owner is required"));
    }
    Ok(())
}
