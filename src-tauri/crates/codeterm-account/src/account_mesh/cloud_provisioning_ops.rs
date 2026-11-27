use super::cloud_enrollment::{fixed_bytes, CloudController};
use super::cloud_provisioning::{
    provisioning_message, ArchiveMachineReply, CloudUsageReply, CreateCloudCreditPackCheckoutReply,
    CreateCloudCreditPackCheckoutRequest, CreateCloudMachineBody, CreateCloudMachineRequest,
    DeleteMachineReply, ManagedCloudMachineActionBody, ManagedCloudMachineActionRequest,
    ProvisionedMachineReply, RecordWarmBootstrapPhasesBody, RecordWarmBootstrapPhasesReply,
    RecordWarmBootstrapPhasesRequest, RedeemCloudCreditPromoReply, RedeemCloudCreditPromoRequest,
    RenewLeaseBody, RenewLeaseReply, RenewLeaseRequest, ResizeCloudVolumeBody,
    ResizeCloudVolumeReply, ResizeCloudVolumeRequest, SleepMachineReply, WireCloudDeleteMode,
    WireCloudStorageTier, WireWarmBootstrapPhase, DOMAIN_CLOUD_ARCHIVE, DOMAIN_CLOUD_CREATE,
    DOMAIN_CLOUD_DELETE, DOMAIN_CLOUD_LEASE_RENEW, DOMAIN_CLOUD_SLEEP, DOMAIN_CLOUD_VOLUME_RESIZE,
    DOMAIN_CLOUD_WAKE, DOMAIN_CLOUD_WARM_PHASES,
};
use super::state::AccountMeshRefusal;

impl CloudController<'_> {
    pub fn account_id(&self) -> &str {
        &self.signed.account
    }

    fn ensure_current(&self) -> Result<(), AccountMeshRefusal> {
        let view = self.mesh.state();
        if view.account_epoch == self.signed.epoch
            && view.current_membership(self.mesh.now()).is_some()
        {
            Ok(())
        } else {
            Err(AccountMeshRefusal::Cancelled)
        }
    }

    pub async fn create_machine(
        &self,
        size: &str,
        controller_key: &str,
        creation_psk: &str,
        request_id: &str,
    ) -> Result<ProvisionedMachineReply, AccountMeshRefusal> {
        self.ensure_current()?;
        if fixed_bytes::<32>(controller_key).is_none()
            || fixed_bytes::<32>(creation_psk).is_none()
            || fixed_bytes::<32>(request_id).is_none()
            || size.trim().is_empty()
        {
            return Err(AccountMeshRefusal::Invalid);
        }
        let body = CreateCloudMachineBody {
            schema_version: 1,
            controller_installation_id: self.mesh.installation(),
            controller_key: controller_key.to_owned(),
            creation_psk: creation_psk.to_owned(),
            size: size.trim().to_owned(),
            request_id: request_id.to_owned(),
            timestamp: self.mesh.now(),
        };
        let message = provisioning_message(
            DOMAIN_CLOUD_CREATE,
            self.signed.trust.project(),
            &self.signed.account,
            &body,
        )
        .ok_or(AccountMeshRefusal::Invalid)?;
        let controller_signature = self.signed.key.sign_cloud_provisioning(&message);
        let request = CreateCloudMachineRequest {
            body,
            controller_signature,
        };
        let reply = self
            .mesh
            .call(
                self.mesh
                    .ports
                    .registry
                    .create_cloud_machine(&self.signed.session, &request),
            )
            .await?;
        drop(request);
        self.ensure_current()?;
        if reply.validated_endpoint().is_none() {
            return Err(AccountMeshRefusal::Unavailable);
        }
        Ok(reply)
    }

    fn build_action_request(
        &self,
        domain: &str,
        machine_id: &str,
        request_id: &str,
    ) -> Result<ManagedCloudMachineActionRequest, AccountMeshRefusal> {
        self.ensure_current()?;
        if machine_id.trim().is_empty() || fixed_bytes::<32>(request_id).is_none() {
            return Err(AccountMeshRefusal::Invalid);
        }
        let body = ManagedCloudMachineActionBody {
            schema_version: 1,
            controller_installation_id: self.mesh.installation(),
            machine_id: machine_id.trim().to_owned(),
            request_id: request_id.to_owned(),
            timestamp: self.mesh.now(),
            mode: None,
            archive_id: None,
            controller_key: None,
            creation_psk: None,
        };
        let message = provisioning_message(
            domain,
            self.signed.trust.project(),
            &self.signed.account,
            &body,
        )
        .ok_or(AccountMeshRefusal::Invalid)?;
        let controller_signature = self.signed.key.sign_cloud_provisioning(&message);
        Ok(ManagedCloudMachineActionRequest {
            body,
            controller_signature,
        })
    }

    pub async fn wake_machine(
        &self,
        machine_id: &str,
        request_id: &str,
    ) -> Result<ProvisionedMachineReply, AccountMeshRefusal> {
        let request = self.build_action_request(DOMAIN_CLOUD_WAKE, machine_id, request_id)?;
        let reply = self
            .mesh
            .call(
                self.mesh
                    .ports
                    .registry
                    .wake_cloud_machine(&self.signed.session, &request),
            )
            .await?;
        self.ensure_current()?;
        if reply.validated_endpoint().is_none() {
            return Err(AccountMeshRefusal::Unavailable);
        }
        Ok(reply)
    }

    pub async fn sleep_machine(
        &self,
        machine_id: &str,
        request_id: &str,
    ) -> Result<SleepMachineReply, AccountMeshRefusal> {
        let request = self.build_action_request(DOMAIN_CLOUD_SLEEP, machine_id, request_id)?;
        let reply = self
            .mesh
            .call(
                self.mesh
                    .ports
                    .registry
                    .sleep_cloud_machine(&self.signed.session, &request),
            )
            .await?;
        self.ensure_current()?;
        if reply.state != "asleep" || reply.machine_id != machine_id.trim() {
            return Err(AccountMeshRefusal::Unavailable);
        }
        Ok(reply)
    }

    pub async fn archive_machine(
        &self,
        machine_id: &str,
        request_id: &str,
    ) -> Result<ArchiveMachineReply, AccountMeshRefusal> {
        let request = self.build_action_request(DOMAIN_CLOUD_ARCHIVE, machine_id, request_id)?;
        let reply = self
            .mesh
            .call(
                self.mesh
                    .ports
                    .registry
                    .archive_cloud_machine(&self.signed.session, &request),
            )
            .await?;
        self.ensure_current()?;
        if reply.storage_tier != WireCloudStorageTier::Cold
            || reply.machine_id != machine_id.trim()
            || reply.archive_id.trim().is_empty()
        {
            return Err(AccountMeshRefusal::Unavailable);
        }
        Ok(reply)
    }

    pub async fn delete_machine(
        &self,
        machine_id: &str,
        request_id: &str,
    ) -> Result<DeleteMachineReply, AccountMeshRefusal> {
        self.delete_machine_with_mode(machine_id, request_id, None, None)
            .await
    }

    pub async fn delete_machine_with_mode(
        &self,
        machine_id: &str,
        request_id: &str,
        mode: Option<WireCloudDeleteMode>,
        archive_id: Option<&str>,
    ) -> Result<DeleteMachineReply, AccountMeshRefusal> {
        self.ensure_current()?;
        if machine_id.trim().is_empty() || fixed_bytes::<32>(request_id).is_none() {
            return Err(AccountMeshRefusal::Invalid);
        }
        let body = ManagedCloudMachineActionBody {
            schema_version: 1,
            controller_installation_id: self.mesh.installation(),
            machine_id: machine_id.trim().to_owned(),
            request_id: request_id.to_owned(),
            timestamp: self.mesh.now(),
            mode,
            archive_id: archive_id
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty()),
            controller_key: None,
            creation_psk: None,
        };
        let message = provisioning_message(
            DOMAIN_CLOUD_DELETE,
            self.signed.trust.project(),
            &self.signed.account,
            &body,
        )
        .ok_or(AccountMeshRefusal::Invalid)?;
        let controller_signature = self.signed.key.sign_cloud_provisioning(&message);
        let request = ManagedCloudMachineActionRequest {
            body,
            controller_signature,
        };
        let reply = self
            .mesh
            .call(
                self.mesh
                    .ports
                    .registry
                    .delete_cloud_machine(&self.signed.session, &request),
            )
            .await?;
        self.ensure_current()?;
        if reply.state != "deleted" || reply.machine_id != machine_id.trim() {
            return Err(AccountMeshRefusal::Unavailable);
        }
        Ok(reply)
    }

    pub async fn wake_machine_cold(
        &self,
        machine_id: &str,
        request_id: &str,
        controller_key: &str,
        creation_psk: &str,
        archive_id: Option<&str>,
    ) -> Result<ProvisionedMachineReply, AccountMeshRefusal> {
        self.ensure_current()?;
        if machine_id.trim().is_empty()
            || fixed_bytes::<32>(request_id).is_none()
            || fixed_bytes::<32>(controller_key).is_none()
            || fixed_bytes::<32>(creation_psk).is_none()
        {
            return Err(AccountMeshRefusal::Invalid);
        }
        let body = ManagedCloudMachineActionBody {
            schema_version: 1,
            controller_installation_id: self.mesh.installation(),
            machine_id: machine_id.trim().to_owned(),
            request_id: request_id.to_owned(),
            timestamp: self.mesh.now(),
            mode: None,
            archive_id: archive_id
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty()),
            controller_key: Some(controller_key.to_owned()),
            creation_psk: Some(creation_psk.to_owned()),
        };
        let message = provisioning_message(
            DOMAIN_CLOUD_WAKE,
            self.signed.trust.project(),
            &self.signed.account,
            &body,
        )
        .ok_or(AccountMeshRefusal::Invalid)?;
        let controller_signature = self.signed.key.sign_cloud_provisioning(&message);
        let request = ManagedCloudMachineActionRequest {
            body,
            controller_signature,
        };
        let reply = self
            .mesh
            .call(
                self.mesh
                    .ports
                    .registry
                    .wake_cloud_machine(&self.signed.session, &request),
            )
            .await?;
        drop(request);
        self.ensure_current()?;
        if reply.validated_endpoint().is_none() {
            return Err(AccountMeshRefusal::Unavailable);
        }
        Ok(reply)
    }

    pub async fn resize_volume(
        &self,
        machine_id: &str,
        new_size_gb: u32,
        confirmed_by_owner: bool,
        request_id: &str,
    ) -> Result<ResizeCloudVolumeReply, AccountMeshRefusal> {
        self.ensure_current()?;
        if !confirmed_by_owner
            || machine_id.trim().is_empty()
            || new_size_gb == 0
            || new_size_gb > 500
            || fixed_bytes::<32>(request_id).is_none()
        {
            return Err(AccountMeshRefusal::Invalid);
        }
        let body = ResizeCloudVolumeBody {
            schema_version: 1,
            controller_installation_id: self.mesh.installation(),
            machine_id: machine_id.trim().to_owned(),
            new_size_gb,
            confirmed_by_owner: true,
            request_id: request_id.to_owned(),
            timestamp: self.mesh.now(),
        };
        let message = provisioning_message(
            DOMAIN_CLOUD_VOLUME_RESIZE,
            self.signed.trust.project(),
            &self.signed.account,
            &body,
        )
        .ok_or(AccountMeshRefusal::Invalid)?;
        let controller_signature = self.signed.key.sign_cloud_provisioning(&message);
        let request = ResizeCloudVolumeRequest {
            body,
            controller_signature,
        };
        let reply = self
            .mesh
            .call(
                self.mesh
                    .ports
                    .registry
                    .resize_cloud_volume(&self.signed.session, &request),
            )
            .await?;
        self.ensure_current()?;
        Ok(reply)
    }

    pub async fn record_warm_bootstrap_phases(
        &self,
        machine_id: &str,
        phase: WireWarmBootstrapPhase,
        request_id: &str,
    ) -> Result<RecordWarmBootstrapPhasesReply, AccountMeshRefusal> {
        self.ensure_current()?;
        let trimmed_machine = machine_id.trim();
        if trimmed_machine.is_empty() || fixed_bytes::<32>(request_id).is_none() {
            return Err(AccountMeshRefusal::Invalid);
        }
        let body = RecordWarmBootstrapPhasesBody {
            schema_version: 1,
            controller_installation_id: self.mesh.installation(),
            machine_id: trimmed_machine.to_owned(),
            phase,
            request_id: request_id.to_owned(),
            timestamp: self.mesh.now(),
        };
        let message = provisioning_message(
            DOMAIN_CLOUD_WARM_PHASES,
            self.signed.trust.project(),
            &self.signed.account,
            &body,
        )
        .ok_or(AccountMeshRefusal::Invalid)?;
        let controller_signature = self.signed.key.sign_cloud_provisioning(&message);
        let request = RecordWarmBootstrapPhasesRequest {
            body,
            controller_signature,
        };
        let reply = self
            .mesh
            .call(
                self.mesh
                    .ports
                    .registry
                    .record_warm_bootstrap_phases(&self.signed.session, &request),
            )
            .await?;
        self.ensure_current()?;
        if reply.machine_id != trimmed_machine {
            return Err(AccountMeshRefusal::Unavailable);
        }
        Ok(reply)
    }

    pub async fn renew_lease(
        &self,
        machine_id: &str,
        lease_id: &str,
        request_id: &str,
    ) -> Result<RenewLeaseReply, AccountMeshRefusal> {
        self.ensure_current()?;
        if machine_id.trim().is_empty()
            || lease_id.trim().is_empty()
            || fixed_bytes::<32>(request_id).is_none()
        {
            return Err(AccountMeshRefusal::Invalid);
        }
        let body = RenewLeaseBody {
            schema_version: 1,
            controller_installation_id: self.mesh.installation(),
            machine_id: machine_id.trim().to_owned(),
            lease_id: lease_id.trim().to_owned(),
            request_id: request_id.to_owned(),
            timestamp: self.mesh.now(),
        };
        let message = provisioning_message(
            DOMAIN_CLOUD_LEASE_RENEW,
            self.signed.trust.project(),
            &self.signed.account,
            &body,
        )
        .ok_or(AccountMeshRefusal::Invalid)?;
        let controller_signature = self.signed.key.sign_cloud_provisioning(&message);
        let request = RenewLeaseRequest {
            body,
            controller_signature,
        };
        let reply = self
            .mesh
            .call(
                self.mesh
                    .ports
                    .registry
                    .renew_cloud_lease(&self.signed.session, &request),
            )
            .await?;
        self.ensure_current()?;
        Ok(reply)
    }

    pub async fn cloud_usage(&self) -> Result<CloudUsageReply, AccountMeshRefusal> {
        self.ensure_current()?;
        let reply = self
            .mesh
            .call(self.mesh.ports.registry.cloud_usage(&self.signed.session))
            .await?;
        self.ensure_current()?;
        Ok(reply)
    }

    pub async fn redeem_promo(
        &self,
        code: &str,
    ) -> Result<RedeemCloudCreditPromoReply, AccountMeshRefusal> {
        self.ensure_current()?;
        let trimmed = code.trim();
        if trimmed.is_empty() || trimmed.len() > 64 {
            return Err(AccountMeshRefusal::PromoInvalidCode);
        }
        let request = RedeemCloudCreditPromoRequest {
            code: trimmed.to_owned(),
        };
        let reply = self
            .mesh
            .call(
                self.mesh
                    .ports
                    .registry
                    .redeem_cloud_promo(&self.signed.session, &request),
            )
            .await?;
        self.ensure_current()?;
        if !reply.is_valid() {
            return Err(AccountMeshRefusal::Unavailable);
        }
        Ok(reply)
    }

    pub async fn create_checkout(
        &self,
        pack_id: &str,
        amount_cents: Option<u64>,
    ) -> Result<CreateCloudCreditPackCheckoutReply, AccountMeshRefusal> {
        self.ensure_current()?;
        let pack_id = pack_id.trim();
        if pack_id.is_empty() {
            return Err(AccountMeshRefusal::Invalid);
        }
        if pack_id == "custom" {
            if !amount_cents.is_some_and(|cents| cents > 0) {
                return Err(AccountMeshRefusal::Invalid);
            }
        } else if amount_cents.is_some() {
            return Err(AccountMeshRefusal::Invalid);
        }
        let request = CreateCloudCreditPackCheckoutRequest {
            pack_id: pack_id.to_owned(),
            amount_cents,
        };
        let reply = self
            .mesh
            .call(
                self.mesh
                    .ports
                    .registry
                    .create_cloud_checkout(&self.signed.session, &request),
            )
            .await?;
        self.ensure_current()?;
        if !reply.is_valid() {
            return Err(AccountMeshRefusal::Unavailable);
        }
        Ok(reply)
    }
}
