use super::lifecycle::Inner;
use super::state::KeyBinding;
use super::wire::MeshManagementRow;

impl Inner {
    pub(super) fn observe_registry_keys(&mut self, rows: &[MeshManagementRow]) -> bool {
        let mut changed = false;
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
            if row.active
                && self
                    .record
                    .revoked_installations
                    .contains(&row.installation_id)
                && self
                    .record
                    .revoked_generations
                    .get(&row.installation_id)
                    .is_none_or(|revoked| row.key_generation > *revoked)
            {
                self.record
                    .revoked_installations
                    .remove(&row.installation_id);
                changed = true;
            }
        }
        changed
    }
}
