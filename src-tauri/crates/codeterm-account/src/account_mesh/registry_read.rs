use super::lifecycle::AccountMesh;
use super::state::{AccountMeshRefusal, RegistryObservation};
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
    pub async fn list(&self) -> Outcome {
        self.registry_list(true).await
    }

    pub(super) async fn list_fresh(&self) -> Outcome {
        self.registry_list(false).await
    }

    async fn registry_list(&self, allow_cached: bool) -> Outcome {
        let signed = self.signed(None).await?;
        if let Some(cached) = self.with(|inner| {
            (allow_cached && inner.is_current(signed.epoch)
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
