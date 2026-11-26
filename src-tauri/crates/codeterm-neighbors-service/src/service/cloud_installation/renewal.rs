use super::{CloudInstallation, DetachedGrant, EnrollmentError, Phase, Stored};
use codeterm_account::account_mesh::{
    cloud_enrollment::verify_cloud_renewal, registry_snapshot::verify_registry, verify_membership,
    MembershipExpectation,
};

impl codeterm_mesh::cloud_ingress::RenewalReceiver for CloudInstallation {
    fn enrolled(&self) -> bool {
        matches!(&*self.0.phase.lock(), Phase::Enrolled { .. })
    }

    fn apply<'a>(
        &'a self,
        membership: &'a DetachedGrant,
        registry: &'a DetachedGrant,
    ) -> codeterm_mesh::account_mesh::BoxFuture<
        'a,
        Result<(), codeterm_mesh::cloud_ingress::IngressError>,
    > {
        Box::pin(async move {
            self.apply_renewal(membership, registry)
                .await
                .map_err(|_| codeterm_mesh::cloud_ingress::IngressError::Rejected)
        })
    }
}

impl CloudInstallation {
    /// Consumes the grant pair delivered by an admitted renewal frame.
    pub async fn apply_renewal(
        &self,
        membership: &DetachedGrant,
        signed_registry: &DetachedGrant,
    ) -> Result<(), EnrollmentError> {
        let owner = self.clone();
        let writer = self.0.renewal.clone().lock_owned().await;
        let membership = membership.clone();
        let registry = signed_registry.clone();
        tokio::spawn(async move {
            let _writer = writer;
            let worker = owner.clone();
            tokio::task::spawn_blocking(move || worker.persist_renewal(membership, registry))
                .await
                .map_err(|_| EnrollmentError::Unavailable)??;
            owner.publish().await
        })
        .await
        .map_err(|_| EnrollmentError::Unavailable)?
    }

    fn persist_renewal(
        &self,
        membership: DetachedGrant,
        registry: DetachedGrant,
    ) -> Result<(), EnrollmentError> {
        use EnrollmentError::{Rejected, Unavailable};
        let lifecycle = self.0.lifecycle.borrow().clone().ok_or(Unavailable)?;
        let now = codeterm_service_core::service::neighbor_account::now();
        let live = lifecycle.state();
        let active = &live.current_membership(now).ok_or(Rejected)?.verified;
        let id = self.installation_id();
        let key = self.0.signer.key_id();
        if active.installation_id != id
            || active.key_id != key
            || live.revoked_installations.contains(id)
        {
            return Err(Rejected);
        }
        let expected = MembershipExpectation {
            account_id: &active.account_id,
            installation_id: Some(id),
            key_id: Some(&key),
            key_generation: Some(active.key_generation),
            slot_revision: Some(active.slot_revision),
        };
        let known = live.signed_registry.as_ref().ok_or(Rejected)?;
        let (renewed, revision) =
            verify_cloud_renewal(&membership, &registry, &self.0.trust, active, known, now)
                .ok_or(Rejected)?;
        let (account_id, previous, previous_registry) = match &*self.0.phase.lock() {
            Phase::Enrolled {
                account_id,
                membership,
                signed_registry,
            } if account_id == &active.account_id => (
                account_id.clone(),
                membership.clone(),
                signed_registry.clone(),
            ),
            _ => return Err(Rejected),
        };
        if previous == membership && previous_registry == registry {
            return Ok(());
        }
        let prior =
            verify_membership(&previous, &self.0.trust, &expected, now).map_err(|_| Rejected)?;
        let prior_registry = verify_registry(&previous_registry, &self.0.trust, &account_id, now)
            .map_err(|_| Rejected)?;
        if revision <= prior_registry.revision
            || renewed.issued_at < prior.issued_at
            || renewed.expires_at < prior.expires_at
        {
            return Err(Rejected);
        }
        let receipt = Stored::Enrolled {
            account_id: account_id.clone(),
            discovery: self.0.discovery.clone(),
            membership: membership.clone(),
            signed_registry: registry.clone(),
        };
        Self::persist(&self.0.store, &self.0.root, &receipt)?;
        *self.0.phase.lock() = Phase::Enrolled {
            account_id,
            membership,
            signed_registry: registry,
        };
        Ok(())
    }
}
