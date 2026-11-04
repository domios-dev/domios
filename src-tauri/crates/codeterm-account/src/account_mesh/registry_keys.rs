use super::lifecycle::Inner;
use super::state::{AccountFences, KeyBinding, RegistryPresenceBinding};
use super::wire::{MeshManagementRow, RegistryPresence};

impl Inner {
    /// Activates `account`'s rollback fences and revocations, stashing the previous account's.
    /// An account without a stash seeds them from its own last verified registry rows.
    pub(super) fn select_fences(
        &mut self,
        account: &str,
        seed: Option<&[MeshManagementRow]>,
        own: &str,
    ) {
        if self.record.fence_account.as_deref() == Some(account) {
            return;
        }
        let active = AccountFences {
            peer_keys: std::mem::take(&mut self.record.peer_keys),
            peer_presence: std::mem::take(&mut self.record.peer_presence),
            revoked_installations: std::mem::take(&mut self.record.revoked_installations),
            revoked_generations: std::mem::take(&mut self.record.revoked_generations),
        };
        if let Some(previous) = self.record.fence_account.take() {
            self.record.account_fences.insert(previous, active);
        }
        match self.record.account_fences.remove(account) {
            Some(fences) => {
                self.record.peer_keys = fences.peer_keys;
                self.record.peer_presence = fences.peer_presence;
                self.record.revoked_installations = fences.revoked_installations;
                self.record.revoked_generations = fences.revoked_generations;
            }
            None => {
                if let Some(rows) = seed {
                    self.observe_registry_keys(rows, own);
                }
            }
        }
        self.record.fence_account = Some(account.to_owned());
    }

    pub(super) fn observe_registry_keys(&mut self, rows: &[MeshManagementRow], own: &str) -> bool {
        let mut changed = self.record.peer_presence.remove(own).is_some();
        for row in rows {
            if row.key_generation == 0 {
                continue;
            }
            let known = self.record.peer_keys.get(&row.installation_id);
            if known.is_some_and(|known| {
                row.key_generation < known.generation
                    || (row.key_generation == known.generation && row.key_id != known.key_id)
            }) {
                continue;
            }
            if known.is_none_or(|known| row.key_generation > known.generation) {
                self.record.peer_keys.insert(
                    row.installation_id.clone(),
                    KeyBinding {
                        key_id: row.key_id.clone(),
                        generation: row.key_generation,
                    },
                );
                changed = true;
            }
            if row.installation_id != own {
                let presence = self.record.peer_presence.get(&row.installation_id);
                if presence.is_none_or(|known| {
                    row.key_generation > known.generation
                        || (row.key_generation == known.generation
                            && (row.metadata_revision > known.metadata_revision
                                || (row.metadata_revision == known.metadata_revision
                                    && row.presence == RegistryPresence::SignedOut)))
                }) {
                    let observation = RegistryPresenceBinding {
                        generation: row.key_generation,
                        metadata_revision: row.metadata_revision,
                        presence: row.presence,
                    };
                    if presence != Some(&observation) {
                        self.record
                            .peer_presence
                            .insert(row.installation_id.clone(), observation);
                        changed = true;
                    }
                }
            }
            if !row.active {
                let generation = self
                    .record
                    .revoked_generations
                    .entry(row.installation_id.clone())
                    .or_default();
                if *generation < row.key_generation {
                    *generation = row.key_generation;
                    changed = true;
                }
            }
            let blocked = self
                .record
                .revoked_generations
                .get(&row.installation_id)
                .is_some_and(|generation| row.key_generation <= *generation);
            if blocked {
                changed |= self
                    .record
                    .revoked_installations
                    .insert(row.installation_id.clone());
            } else if row.active {
                changed |= self
                    .record
                    .revoked_installations
                    .remove(&row.installation_id);
            }
        }
        changed
    }
}
