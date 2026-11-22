use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::store::{
    encoded_record_path, invalid_data, invalid_input, read_private, refused, validate_record_id,
    validate_tab_id, write_private, MAX_RECORD_BYTES,
};
use super::{
    BindingTarget, ConciergeEpoch, EpochArchive, EpochHistory, TranscriptOwnerState,
    TranscriptStore,
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct EpochDocument {
    owner_tab_id: String,
    pub(super) activation_revision: u64,
    pub(super) history: EpochHistory,
}

impl TranscriptStore {
    pub fn stage_epoch(&self, expected_revision: u64, epoch: ConciergeEpoch) -> io::Result<()> {
        validate_epoch(&epoch)?;
        let mut state = self.inner.state.lock();
        state.require_healthy()?;
        state.binding.require_open()?;
        state.binding.require_revision(expected_revision)?;
        let owner_tab_id = &state.binding.binding.owner_tab_id;
        if let Some(existing) = read_epoch(&self.inner.root, owner_tab_id, &epoch.epoch_id)? {
            if existing.activation_revision == expected_revision
                && existing.history.epoch == epoch
                && existing.history.archive.is_none()
            {
                return Ok(());
            }
            return Err(refused("Transcript epoch identity is already committed"));
        }
        let path = epoch_path(&self.inner.root, &epoch.epoch_id);
        let record = EpochDocument {
            owner_tab_id: owner_tab_id.clone(),
            activation_revision: expected_revision,
            history: EpochHistory {
                epoch,
                archive: None,
            },
        };
        if let Err(error) = write_private(&path, &record, MAX_RECORD_BYTES) {
            state.fence_writes();
            return Err(error);
        }
        Ok(())
    }

    pub fn epoch(&self, epoch_id: &str) -> io::Result<Option<EpochHistory>> {
        let state = self.inner.state.lock();
        state.require_healthy()?;
        Ok(read_epoch(
            &self.inner.root,
            &state.binding.binding.owner_tab_id,
            epoch_id,
        )?
        .map(|document| document.history))
    }

    pub fn archive_epoch(&self, epoch: &ConciergeEpoch, archive: EpochArchive) -> io::Result<()> {
        let mut state = self.inner.state.lock();
        state.require_healthy()?;
        let mut document = require_epoch(
            &self.inner.root,
            &state.binding.binding.owner_tab_id,
            &epoch.epoch_id,
        )?;
        if document.history.epoch != *epoch {
            return Err(refused("Transcript epoch identity mismatch"));
        }
        if matches!(&state.binding.owner, TranscriptOwnerState::Open { .. })
            && matches!(&state.binding.binding.target, BindingTarget::Attached { epoch_id } if epoch_id == &epoch.epoch_id)
        {
            return Err(refused(
                "Fence the active transcript epoch before archiving",
            ));
        }
        if archive.history_ref.is_empty()
            || archive.history_ref.len() > 4096
            || archive.ended_at_ms < epoch.started_at_ms
        {
            return Err(invalid_input("Invalid transcript epoch archive"));
        }
        if let Some(existing) = document.history.archive {
            return if existing == archive {
                Ok(())
            } else {
                Err(refused(
                    "Transcript epoch archive identity is already committed",
                ))
            };
        }
        document.history.archive = Some(archive);
        if let Err(error) = write_private(
            &epoch_path(&self.inner.root, &epoch.epoch_id),
            &document,
            MAX_RECORD_BYTES,
        ) {
            state.fence_writes();
            return Err(error);
        }
        Ok(())
    }
}

pub(super) fn require_epoch(
    root: &Path,
    owner_tab_id: &str,
    epoch_id: &str,
) -> io::Result<EpochDocument> {
    read_epoch(root, owner_tab_id, epoch_id)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Transcript epoch is missing"))
}

fn read_epoch(
    root: &Path,
    owner_tab_id: &str,
    epoch_id: &str,
) -> io::Result<Option<EpochDocument>> {
    validate_record_id(epoch_id)?;
    let record = read_private::<EpochDocument>(&epoch_path(root, epoch_id), MAX_RECORD_BYTES)?;
    if let Some(record) = &record {
        if record.owner_tab_id != owner_tab_id || record.history.epoch.epoch_id != epoch_id {
            return Err(invalid_data("Transcript epoch identity mismatch"));
        }
        validate_epoch(&record.history.epoch)?;
    }
    Ok(record)
}

fn epoch_path(root: &Path, epoch_id: &str) -> PathBuf {
    encoded_record_path(&root.join("concierge_epochs"), epoch_id)
}

fn validate_epoch(epoch: &ConciergeEpoch) -> io::Result<()> {
    validate_record_id(&epoch.epoch_id)?;
    validate_tab_id(&epoch.runtime_tab_id)?;
    if [&epoch.session_id, &epoch.binding_nonce, &epoch.provider]
        .iter()
        .any(|value| value.is_empty() || value.len() > 512)
        || epoch.model.as_ref().is_some_and(|model| model.len() > 512)
    {
        return Err(invalid_input("Invalid transcript epoch runtime identity"));
    }
    Ok(())
}
