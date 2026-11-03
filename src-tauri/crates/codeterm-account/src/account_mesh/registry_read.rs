use super::lifecycle::AccountMesh;
use super::operations::Signed;
use super::state::{AccountMeshRefusal, RegistryObservation};
use super::wire::{MeshOperation, MeshResult};
use tokio::sync::watch;

const MAX_LIST_PAGES: usize = 8;

type Outcome = Result<RegistryObservation, AccountMeshRefusal>;

pub(super) struct RegistryFlight {
    epoch: u64,
    outcome: watch::Receiver<Option<Outcome>>,
}

struct Owner<'a> {
    mesh: &'a AccountMesh,
    epoch: u64,
}

impl Drop for Owner<'_> {
    fn drop(&mut self) {
        self.mesh.with(|inner| {
            if inner
                .registry_flight
                .as_ref()
                .is_some_and(|flight| flight.epoch == self.epoch)
            {
                inner.registry_flight = None;
            }
        });
    }
}

impl AccountMesh {
    /// One bounded online read of the account registry; a stale observation for display only.
    pub(super) async fn read_registry(&self, signed: &Signed) -> Outcome {
        let mut rows = Vec::new();
        let mut cursor = None;
        let mut registry_revision = 0;
        for _ in 0..MAX_LIST_PAGES {
            let page = match self
                .admission(signed, MeshOperation::List { cursor })
                .await?
            {
                MeshResult::Page(page) => page,
                _ => return Err(AccountMeshRefusal::Invalid),
            };
            registry_revision = page.registry_revision;
            rows.extend(page.rows);
            cursor = page.cursor;
            if cursor.is_none() {
                break;
            }
        }
        if let Some(snapshot) = self.state().signed_registry.as_ref() {
            if let Some(own) = snapshot
                .rows
                .iter()
                .find(|row| row.installation_id == self.installation())
            {
                self.remember_metadata(
                    signed.epoch,
                    own.metadata_revision,
                    own.display_name.clone(),
                );
            }
            return Ok(RegistryObservation {
                observed_at: self.now(),
                registry_revision: snapshot.revision,
                rows: snapshot.rows.clone(),
                cursor: None,
            });
        }
        let installation = self.installation();
        if let Some(own) = rows.iter().find(|row| row.installation_id == installation) {
            self.remember_metadata(
                signed.epoch,
                own.metadata_revision,
                own.display_name.clone(),
            );
        }
        let observation = RegistryObservation {
            observed_at: self.now(),
            registry_revision,
            rows,
            cursor,
        };
        let keys_changed = self.with(|inner| {
            if inner.is_current(signed.epoch) || signed.epoch == 0 {
                let recovered_presence =
                    inner.record.peer_presence_refresh && observation.cursor.is_none();
                if recovered_presence {
                    inner.record.peer_presence_refresh = false;
                }
                let changed = recovered_presence
                    | inner.observe_registry_keys(&observation.rows, &installation);
                let revoked = inner.record.revoked_installations.clone();
                let own_invalid = inner.state.membership.as_ref().is_some_and(|membership| {
                    let own = &membership.verified;
                    revoked.contains(&own.installation_id)
                        || inner
                            .record
                            .peer_keys
                            .get(&own.installation_id)
                            .is_some_and(|known| {
                                known.generation > own.key_generation
                                    || (known.generation == own.key_generation
                                        && known.key_id != own.key_id)
                            })
                });
                let slots_changed = inner
                    .state
                    .registry
                    .as_ref()
                    .is_some_and(|known| observation.has_freed_slot_since(known));
                let rearmed = slots_changed && inner.state.plan_limit_locked();
                if rearmed {
                    inner.resume_terminal();
                }
                inner.update(&self.watch, |state| {
                    state.revoked_installations = revoked;
                    if own_invalid {
                        state.membership = None;
                    }
                    state.registry = Some(observation.clone());
                    state.registry_failure = None;
                });
                return changed || rearmed;
            }
            false
        });
        self.push().await;
        if keys_changed {
            self.persist().await;
        }
        Ok(observation)
    }

    pub async fn list(&self) -> Outcome {
        self.registry_list(true).await
    }

    pub async fn list_fresh(&self) -> Outcome {
        self.registry_list(false).await
    }

    async fn registry_list(&self, allow_cached: bool) -> Outcome {
        let signed = self.signed(None).await?;
        if let Some(cached) = self.with(|inner| {
            (allow_cached
                && inner.is_current(signed.epoch)
                && inner.state.enrollment == super::state::Enrollment::Refused
                && matches!(
                    inner.state.last_refusal,
                    Some(AccountMeshRefusal::EnrollmentBudget { .. })
                ))
            .then(|| inner.state.registry.clone())
            .flatten()
        }) {
            return Ok(cached);
        }
        let (sender, mut outcome) = self.with(|inner| {
            if let Some(flight) = inner
                .registry_flight
                .as_ref()
                .filter(|flight| flight.epoch == signed.epoch)
            {
                return (None, flight.outcome.clone());
            }
            let (sender, outcome) = watch::channel(None);
            inner.registry_flight = Some(RegistryFlight {
                epoch: signed.epoch,
                outcome: outcome.clone(),
            });
            (Some(sender), outcome)
        });
        let Some(sender) = sender else {
            let result = match outcome.wait_for(Option::is_some).await {
                Ok(result) => result.clone().unwrap_or(Err(AccountMeshRefusal::Cancelled)),
                Err(_) => Err(AccountMeshRefusal::Cancelled),
            };
            return if self.with(|inner| inner.is_current(signed.epoch)) {
                result
            } else {
                Err(AccountMeshRefusal::Cancelled)
            };
        };
        let _owner = Owner {
            mesh: self,
            epoch: signed.epoch,
        };
        let result = self.read_registry(&signed).await;
        let result = self.with(|inner| {
            if !inner.is_current(signed.epoch) {
                return Err(AccountMeshRefusal::Cancelled);
            }
            if let Err(refusal) = result {
                inner.update(&self.watch, |state| state.registry_failure = Some(refusal));
                return Err(refusal);
            }
            result
        });
        sender.send_replace(Some(result.clone()));
        result
    }
}
