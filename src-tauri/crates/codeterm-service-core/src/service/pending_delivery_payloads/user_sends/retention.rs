use super::{digest, fs, io_error, HashSet, Ordering, UserSendStore};
use crate::multiplexer::state::MultiplexerSnapshot;

pub(crate) struct RestoredPayloadOwners {
    live: HashSet<String>,
    closed: HashSet<String>,
    complete: bool,
}

impl RestoredPayloadOwners {
    pub(crate) fn new(
        snapshot: &MultiplexerSnapshot,
        persisted: Option<&crate::session::store::SessionSnapshot>,
    ) -> Self {
        let persisted_ids = persisted.into_iter().flat_map(|desk| {
            desk.workspaces
                .iter()
                .flat_map(|workspace| workspace.tabs.iter().map(|tab| &tab.id))
        });
        Self {
            live: snapshot
                .tabs
                .keys()
                .chain(persisted_ids)
                .map(|id| digest(id.as_bytes()))
                .collect(),
            closed: snapshot
                .recently_closed
                .iter()
                .map(|row| digest(row.tab.tab_id.as_bytes()))
                .collect(),
            complete: persisted.is_some(),
        }
    }

    fn should_remove(&self, key: &str) -> bool {
        !self.live.contains(key) && (self.complete || self.closed.contains(key))
    }
}

impl UserSendStore {
    pub(crate) fn sweep_closed_tabs(&self, owners: &RestoredPayloadOwners) -> Result<(), String> {
        match fs::symlink_metadata(&self.root) {
            Ok(metadata) => super::validate_owner_directory(&metadata)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(io_error(error)),
        }
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(io_error(error)),
        };
        for entry in entries {
            let entry = entry.map_err(io_error)?;
            let key = entry.file_name();
            let Some(key) = key.to_str() else { continue };
            if key.len() == 64
                && key.bytes().all(|byte| byte.is_ascii_hexdigit())
                && owners.should_remove(key)
                && entry.file_type().map_err(io_error)?.is_dir()
            {
                fs::remove_dir_all(entry.path()).map_err(io_error)?;
            }
        }
        Ok(())
    }

    pub(crate) fn purge_closed_tab(&self, tab_id: &str) -> Result<(), String> {
        let tab = self.tab(tab_id);
        let mut data = tab.data.lock();
        tab.revoked.store(true, Ordering::Release);
        tab.purged.store(true, Ordering::Release);
        *data = None;
        match fs::remove_dir_all(&tab.dir) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(io_error(error)),
        }
    }
}

#[cfg(test)]
mod tests;
