use super::lifecycle::AccountMesh;
use super::operations::Signed;
use super::state::{AccountMeshRefusal, RegistryObservation};
use super::wire::{MeshOperation, MeshResult};
use tokio::sync::watch;

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
        let MeshResult::Page(page) = self
            .admission(signed, MeshOperation::List { cursor: None })
            .await?
        else {
            return Err(AccountMeshRefusal::Invalid);
        };
        let snapshot = if signed.epoch == 0 {
            let grant = page.signed_registry.ok_or(AccountMeshRefusal::Invalid)?;
            let trust = signed.trust.clone();
            let account = signed.account.clone();
            let now = self.now();
            tokio::task::spawn_blocking(move || {
                super::registry_snapshot::verify_registry(&grant, &trust, &account, now)
            })
            .await
            .map_err(|_| AccountMeshRefusal::Invalid)??
        } else {
            self.state()
                .signed_registry
                .clone()
                .ok_or(AccountMeshRefusal::Invalid)?
        };
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
        Ok(RegistryObservation {
            observed_at: self.now(),
            registry_revision: snapshot.revision,
            rows: snapshot.rows,
            cursor: None,
        })
    }

    pub async fn list(&self) -> Outcome {
        self.registry_list(true).await
    }

    pub async fn list_fresh(&self) -> Outcome {
        self.registry_list(false).await
    }

    async fn registry_list(&self, allow_cached: bool) -> Outcome {
        let state = self.state();
        if matches!(state.presence, super::state::Presence::SignedOut) {
            return Err(AccountMeshRefusal::SignedOut);
        }
        if state.last_refusal == Some(AccountMeshRefusal::ReauthRequired) {
            return Err(AccountMeshRefusal::ReauthRequired);
        }
        if self.with(|inner| inner.auth_parked.is_some()) {
            return Err(AccountMeshRefusal::Unauthenticated);
        }
        if allow_cached {
            if let Some(registry) = state.signed_registry.as_ref() {
                return Ok(RegistryObservation {
                    observed_at: state
                        .registry
                        .as_ref()
                        .map_or(registry.issued_at, |known| known.observed_at),
                    registry_revision: registry.revision,
                    rows: registry.rows.clone(),
                    cursor: None,
                });
            }
            return Err(AccountMeshRefusal::Unavailable);
        }
        let signed = self.signed(None).await?;
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
