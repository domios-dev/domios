use super::lifecycle::Inner;
use super::state::{KeyBinding, RegistryPresenceBinding};
use super::wire::{MeshManagementRow, RegistryPresence};

impl Inner {
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
