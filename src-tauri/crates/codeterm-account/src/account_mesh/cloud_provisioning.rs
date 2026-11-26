use std::collections::BTreeMap;
use std::fmt;
use std::net::{Ipv4Addr, SocketAddrV4};

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use super::cloud_enrollment::{fixed_bytes, CloudController};
use super::state::AccountMeshRefusal;

pub const ROUTE_CLOUD_CREATE: &str = "createManagedCloudMachine";
pub const ROUTE_CLOUD_WAKE: &str = "wakeManagedCloudMachine";
pub const ROUTE_CLOUD_SLEEP: &str = "sleepManagedCloudMachine";
pub const ROUTE_CLOUD_DELETE: &str = "deleteManagedCloudMachine";
pub const ROUTE_CLOUD_LEASE_RENEW: &str = "renewManagedCloudLease";
pub const ROUTE_CLOUD_USAGE: &str = "getManagedCloudUsage";

pub const DOMAIN_CLOUD_CREATE: &str = "domios.cloud-create.v1";
pub const DOMAIN_CLOUD_WAKE: &str = "domios.cloud-wake.v1";
pub const DOMAIN_CLOUD_SLEEP: &str = "domios.cloud-sleep.v1";
pub const DOMAIN_CLOUD_DELETE: &str = "domios.cloud-delete.v1";
pub const DOMAIN_CLOUD_LEASE_RENEW: &str = "domios.cloud-lease-renew.v1";

pub const CLOUD_INGRESS_PORT: u16 = 7687;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WireCloudSleepMode {
    Pause,
    Stop,
    StopBilled,
    DeleteRecreate,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WireCloudManagedRate {
    pub amount: f64,
    pub currency: String,
    pub basis: String,
    pub credits_per_hour: f64,
}

impl WireCloudManagedRate {
    pub fn is_valid(&self) -> bool {
        self.basis == "managed"
            && self.amount.is_finite()
            && self.amount >= 0.0
            && !self.currency.trim().is_empty()
            && self.credits_per_hour.is_finite()
            && self.credits_per_hour > 0.0
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateCloudMachineBody {
    pub schema_version: u8,
    pub controller_installation_id: String,
    pub controller_key: String,
    pub creation_psk: String,
    pub size: String,
    pub request_id: String,
    pub timestamp: u64,
}

impl Drop for CreateCloudMachineBody {
    fn drop(&mut self) {
        self.creation_psk.zeroize();
    }
}

impl fmt::Debug for CreateCloudMachineBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CreateCloudMachineBody")
            .field("schema_version", &self.schema_version)
            .field(
                "controller_installation_id",
                &self.controller_installation_id,
            )
            .field("controller_key", &self.controller_key)
            .field("creation_psk", &"<redacted>")
            .field("size", &self.size)
            .field("request_id", &self.request_id)
            .field("timestamp", &self.timestamp)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateCloudMachineRequest {
    pub body: CreateCloudMachineBody,
    pub controller_signature: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedCloudMachineActionBody {
    pub schema_version: u8,
    pub controller_installation_id: String,
    pub machine_id: String,
    pub request_id: String,
    pub timestamp: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManagedCloudMachineActionRequest {
    pub body: ManagedCloudMachineActionBody,
    pub controller_signature: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProvisionedMachineReply {
    pub machine_id: String,
    pub provider: String,
    pub ipv4: String,
    pub port: u16,
    pub size: String,
    pub rate: WireCloudManagedRate,
    pub sleep_mode: WireCloudSleepMode,
    pub state: String,
    pub lease_id: String,
    pub lease_expires_at: u64,
}

impl ProvisionedMachineReply {
    pub fn validated_endpoint(&self) -> Option<SocketAddrV4> {
        if self.port != CLOUD_INGRESS_PORT
            || self.state != "awake"
            || self.machine_id.trim().is_empty()
            || self.provider.trim().is_empty()
            || self.size.trim().is_empty()
            || self.lease_id.trim().is_empty()
            || !self.rate.is_valid()
        {
            return None;
        }
        let ip: Ipv4Addr = self.ipv4.parse().ok()?;
        Some(SocketAddrV4::new(ip, self.port))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SleepMachineReply {
    pub machine_id: String,
    pub sleep_mode: WireCloudSleepMode,
    pub state: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeleteMachineReply {
    pub machine_id: String,
    pub state: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenewLeaseBody {
    pub schema_version: u8,
    pub controller_installation_id: String,
    pub machine_id: String,
    pub lease_id: String,
    pub request_id: String,
    pub timestamp: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenewLeaseRequest {
    pub body: RenewLeaseBody,
    pub controller_signature: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenewLeaseReply {
    pub machine_id: String,
    pub lease_id: String,
    pub lease_expires_at: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloudUsageReply {
    pub used: f64,
    pub allowance: f64,
    pub purchased: f64,
    pub remaining: f64,
    pub cap: f64,
    #[serde(default)]
    pub burn_per_size: BTreeMap<String, f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub currency: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub period_end_at: Option<u64>,
}

/// Typed refusal detail matching `shared/cloud-provisioning.ts::CloudProvisioningRefusal`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", deny_unknown_fields)]
pub enum CloudProvisioningRefusal {
    #[serde(rename = "invalid")]
    Invalid,
    #[serde(rename = "unauthenticated")]
    Unauthenticated,
    #[serde(rename = "app-check")]
    AppCheck,
    #[serde(rename = "session_revoked")]
    SessionRevoked,
    #[serde(rename = "forbidden")]
    Forbidden,
    #[serde(rename = "revoked")]
    Revoked,
    #[serde(rename = "proof_invalid")]
    ProofInvalid,
    #[serde(rename = "cloud.machines")]
    CloudMachines { limit: u32, active_count: u32 },
    #[serde(rename = "lease_required")]
    LeaseRequired,
    #[serde(rename = "credits_exhausted")]
    CreditsExhausted,
    #[serde(rename = "not_found")]
    NotFound,
    #[serde(rename = "conflict")]
    Conflict,
    #[serde(rename = "replay")]
    Replay,
    #[serde(rename = "rate-limited")]
    RateLimited { retry_after_seconds: u64 },
    #[serde(rename = "provider_refused")]
    ProviderRefused {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider_reason: Option<String>,
    },
    #[serde(rename = "provider_unavailable")]
    ProviderUnavailable,
    #[serde(rename = "provider_unconfirmed")]
    ProviderUnconfirmed,
    #[serde(rename = "authority-unavailable")]
    AuthorityUnavailable,
    #[serde(rename = "unavailable")]
    Unavailable,
}

pub fn provisioning_message<T: Serialize>(
    domain: &str,
    project: &str,
    account_id: &str,
    body: &T,
) -> Option<Vec<u8>> {
    let value = serde_json::json!({
        "domain": domain,
        "project": project,
        "account_id": account_id,
        "body": body,
    });
    codeterm_entitlements::jcs::encode(&value)
        .ok()
        .map(String::into_bytes)
}

impl CloudController<'_> {
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

    pub async fn delete_machine(
        &self,
        machine_id: &str,
        request_id: &str,
    ) -> Result<DeleteMachineReply, AccountMeshRefusal> {
        let request = self.build_action_request(DOMAIN_CLOUD_DELETE, machine_id, request_id)?;
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
}
