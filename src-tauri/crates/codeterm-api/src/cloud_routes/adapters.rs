use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use codeterm_neighbors_service::service::cloud_installation::controller::{
    enroll as baker_enroll, publish_direct_endpoint as ferry_publish_direct_endpoint,
    ControllerError, CreationBinding,
};
use codeterm_service_core::{
    mesh::crown::AuthenticatedOrigin,
    service::cloud_machines::{
        CloudEndpoint, CloudEnrollmentPort, CloudError, CloudMachineRecord, CloudPeerControlPort,
        CloudProvisioningPort, CloudRouteSpec, DrainToken, EnrollmentBinding, EnrollmentOutcome,
        FlushWriteOutcome, HeldCloudSend, PeerBusySnapshot,
    },
};

use crate::routes::AppState;

#[cfg(test)]
pub(super) use super::provisioning_adapter::map_checkout_refusal;

pub(super) struct BakerEnrollmentAdapter {
    pub(super) svc: codeterm_service_core::service::ServiceState,
}

#[async_trait]
impl CloudEnrollmentPort for BakerEnrollmentAdapter {
    async fn prepare_binding(&self) -> Result<EnrollmentBinding, CloudError> {
        let binding = CreationBinding::new(&self.svc)
            .await
            .map_err(|error| CloudError::EnrollmentFailed(format!("{error:?}")))?;
        let controller_key = binding.controller_key();
        let provider_creation_nonce = binding.provider_creation_nonce();
        Ok(EnrollmentBinding {
            controller_key,
            provider_creation_nonce,
            opaque: Arc::new(binding),
        })
    }

    async fn enroll(
        &self,
        route: &CloudRouteSpec,
        binding: &EnrollmentBinding,
        provider: &str,
        size: &str,
    ) -> Result<EnrollmentOutcome, CloudError> {
        let creation = binding
            .opaque
            .downcast_ref::<CreationBinding>()
            .ok_or_else(|| CloudError::EnrollmentFailed("invalid creation binding".into()))?;
        match baker_enroll(&self.svc, route.clone(), creation, provider, size).await {
            Ok(installation_id) => Ok(EnrollmentOutcome::Enrolled {
                installation_id,
                peer_instance_id: None,
            }),
            Err(ControllerError::ApplyUnconfirmed { installation_id }) => {
                Ok(EnrollmentOutcome::ApplyUnconfirmed { installation_id })
            }
            Err(error) => Err(CloudError::EnrollmentFailed(format!("{error:?}"))),
        }
    }
}

/// Optional host extension allowing registered-router tests to substitute a
/// fake `CloudProvisioningPort` without opening a live account-mesh RPC channel.
#[cfg(any(test, feature = "test-support"))]
#[derive(Clone)]
pub struct CloudProvisioningOverride(pub Arc<dyn CloudProvisioningPort>);

/// Optional host extension allowing tests to intercept `publish_direct_endpoint`
/// without opening a live enrolled cloud channel.
#[cfg(any(test, feature = "test-support"))]
#[derive(Clone)]
pub struct CloudEndpointPublisherOverride(
    pub  Arc<
        dyn Fn(
                CloudEndpoint,
                String,
            ) -> codeterm_mesh::account_mesh::BoxFuture<'static, Result<(), String>>
            + Send
            + Sync,
    >,
);

pub struct MeshPeerControlAdapter {
    pub(super) state: AppState,
}

impl MeshPeerControlAdapter {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }

    pub(super) fn peer_client(
        &self,
        record: &CloudMachineRecord,
    ) -> Result<codeterm_service_core::mesh::transport::PeerClient, CloudError> {
        let mesh = self.state.mesh.lock();
        let target = record
            .peer_instance_id
            .as_deref()
            .or(record.installation_id.as_deref())
            .unwrap_or(&record.machine_id);
        let peer_id = mesh
            .resolve_peer(target)
            .map(|peer| peer.instance_id.clone())
            .map_err(|_| CloudError::ProviderUnavailable(format!("peer {target} is not online")))?;
        mesh.client_for(&peer_id)
            .ok_or_else(|| CloudError::ProviderUnavailable(format!("peer {target} has no route")))
    }

    pub(super) fn resolve_installation_id(&self, record: &CloudMachineRecord) -> Option<String> {
        let mesh = self.state.mesh.lock();
        for selector in [
            record.peer_instance_id.as_deref(),
            record.installation_id.as_deref(),
            Some(record.machine_id.as_str()),
        ]
        .into_iter()
        .flatten()
        {
            if let Ok(peer) = mesh.resolve_peer_including_offline(selector) {
                if let Some(id) = mesh
                    .link_record(&peer.instance_id)
                    .and_then(|link| link.installation_id())
                {
                    return Some(id.to_owned());
                }
            }
        }
        record.installation_id.clone()
    }

    fn cloud_ingress_route(
        &self,
        record: &CloudMachineRecord,
    ) -> Option<(String, String, Option<CloudEndpoint>)> {
        let mesh = self.state.mesh.lock();
        let mut resolved_peer_id = None;
        for selector in [
            record.peer_instance_id.as_deref(),
            record.installation_id.as_deref(),
            Some(record.machine_id.as_str()),
        ]
        .into_iter()
        .flatten()
        {
            if let Ok(peer) = mesh.resolve_peer_including_offline(selector) {
                resolved_peer_id = Some(peer.instance_id.clone());
                break;
            }
        }
        let peer_id = resolved_peer_id?;
        if !mesh.traffic_open(&peer_id) {
            return None;
        }
        let installation_id = mesh
            .link_record(&peer_id)
            .and_then(|link| link.installation_id())
            .map(str::to_owned)
            .or_else(|| record.installation_id.clone())?;
        let known_endpoint = record.endpoint.clone();
        if known_endpoint.is_none() && mesh.peer_route_endpoints(&peer_id).is_some() {
            return None;
        }
        Some((peer_id, installation_id, known_endpoint))
    }

    async fn send_cloud_relay_with_token(
        &self,
        record: &CloudMachineRecord,
        cmd: codeterm_service_core::mesh::types::RelayCommand,
        token: String,
    ) -> Option<
        Result<
            codeterm_service_core::mesh::types::RelayResponse,
            crate::mesh_routes::relay::CloudIngressSendError,
        >,
    > {
        let (peer_id, installation_id, known) = self.cloud_ingress_route(record)?;
        let endpoint = crate::mesh_routes::relay::resolve_cloud_ingress_endpoint(
            &self.state,
            &peer_id,
            &installation_id,
            known,
        )?;
        Some(
            crate::mesh_routes::relay::send_via_cloud_ingress(
                &self.state,
                endpoint,
                &installation_id,
                cmd,
                token,
            )
            .await,
        )
    }

    pub(super) async fn send_controller_cloud_relay(
        &self,
        record: &CloudMachineRecord,
        cmd: codeterm_service_core::mesh::types::RelayCommand,
    ) -> Option<
        Result<
            codeterm_service_core::mesh::types::RelayResponse,
            crate::mesh_routes::relay::CloudIngressSendError,
        >,
    > {
        let token =
            crate::mesh_routes::relay::sign_caller(&self.state, AuthenticatedOrigin::Mesh, None)
                .ok()?;
        self.send_cloud_relay_with_token(record, cmd, token).await
    }

    fn peer_is_online(&self, record: &CloudMachineRecord) -> bool {
        let mesh = self.state.mesh.lock();
        let target = record
            .peer_instance_id
            .as_deref()
            .or(record.installation_id.as_deref())
            .unwrap_or(&record.machine_id);
        if mesh.resolve_peer(target).is_ok_and(|peer| peer.online) {
            return true;
        }
        let Ok(peer) = mesh.resolve_peer_including_offline(target) else {
            return false;
        };
        if !mesh.traffic_open(&peer.instance_id) {
            return false;
        }
        let installation_id = mesh
            .link_record(&peer.instance_id)
            .and_then(|link| link.installation_id())
            .or(record.installation_id.as_deref());
        installation_id.is_some_and(|id| mesh.cloud_ingress_endpoint(id).is_some())
    }
}

#[async_trait]
impl CloudPeerControlPort for MeshPeerControlAdapter {
    async fn read_idle(&self, record: &CloudMachineRecord) -> Result<PeerBusySnapshot, CloudError> {
        let value = if let Some(cloud_result) = self
            .send_controller_cloud_relay(
                record,
                codeterm_service_core::mesh::types::RelayCommand {
                    method: "GET".into(),
                    path: "/api/mesh/cloud/peer/idle".into(),
                    body: serde_json::Value::Null,
                    delivery_id: None,
                },
            )
            .await
        {
            let resp =
                cloud_result.map_err(|error| CloudError::ProviderUnavailable(error.to_string()))?;
            if !resp.ok {
                return Err(CloudError::ProviderUnavailable(resp.error.unwrap_or_else(
                    || format!("cloud peer idle returned status {}", resp.status),
                )));
            }
            resp.data.unwrap_or(serde_json::Value::Null)
        } else {
            let client = self.peer_client(record)?;
            client
                .auth_get_json("/api/mesh/cloud/peer/idle")
                .await
                .map_err(CloudError::ProviderUnavailable)?
        };
        serde_json::from_value(value).map_err(|error| CloudError::Internal(error.to_string()))
    }

    async fn enter_drain(&self, record: &CloudMachineRecord) -> Result<DrainToken, CloudError> {
        let value = if let Some(cloud_result) = self
            .send_controller_cloud_relay(
                record,
                codeterm_service_core::mesh::types::RelayCommand {
                    method: "POST".into(),
                    path: "/api/mesh/cloud/peer/drain".into(),
                    body: serde_json::json!({}),
                    delivery_id: None,
                },
            )
            .await
        {
            let resp =
                cloud_result.map_err(|error| CloudError::ProviderUnavailable(error.to_string()))?;
            if !resp.ok {
                let detail = resp
                    .error
                    .unwrap_or_else(|| format!("status {}", resp.status));
                return Err(if resp.status == 409 || detail.contains("machine_busy") {
                    CloudError::MachineBusy {
                        machine_id: record.machine_id.clone(),
                        reason: detail,
                    }
                } else {
                    CloudError::ProviderUnavailable(detail)
                });
            }
            resp.data.unwrap_or(serde_json::Value::Null)
        } else {
            let client = self.peer_client(record)?;
            client
                .auth_post_json("/api/mesh/cloud/peer/drain", &serde_json::json!({}))
                .await
                .map_err(|error| {
                    if error.contains("machine_busy") || error.contains("409") {
                        CloudError::MachineBusy {
                            machine_id: record.machine_id.clone(),
                            reason: error,
                        }
                    } else {
                        CloudError::ProviderUnavailable(error)
                    }
                })?
        };
        serde_json::from_value(value).map_err(|error| CloudError::Internal(error.to_string()))
    }

    async fn reopen_drain(&self, record: &CloudMachineRecord) -> Result<(), CloudError> {
        if let Some(cloud_result) = self
            .send_controller_cloud_relay(
                record,
                codeterm_service_core::mesh::types::RelayCommand {
                    method: "POST".into(),
                    path: "/api/mesh/cloud/peer/reopen".into(),
                    body: serde_json::json!({}),
                    delivery_id: None,
                },
            )
            .await
        {
            let resp =
                cloud_result.map_err(|error| CloudError::ProviderUnavailable(error.to_string()))?;
            return if resp.ok {
                Ok(())
            } else {
                Err(CloudError::ProviderUnavailable(resp.error.unwrap_or_else(
                    || format!("cloud peer reopen returned status {}", resp.status),
                )))
            };
        }
        let client = self.peer_client(record)?;
        client
            .auth_post_json("/api/mesh/cloud/peer/reopen", &serde_json::json!({}))
            .await
            .map(|_| ())
            .map_err(CloudError::ProviderUnavailable)
    }

    async fn update_endpoint(
        &self,
        record: &CloudMachineRecord,
        endpoint: &CloudEndpoint,
    ) -> Result<(), CloudError> {
        if let Some(installation_id) = self.resolve_installation_id(record) {
            self.state
                .mesh
                .lock()
                .pin_managed_cloud_endpoint(installation_id, endpoint.clone());
        }
        let CloudEndpoint::Direct(address) = endpoint else {
            return Ok(());
        };
        let mut mesh = self.state.mesh.lock();
        let target = record
            .peer_instance_id
            .as_deref()
            .or(record.installation_id.as_deref())
            .unwrap_or(&record.machine_id);
        if let Ok(peer_id) = mesh
            .resolve_peer_including_offline(target)
            .map(|peer| peer.instance_id.clone())
        {
            if let Err(error) = mesh.set_peer_addr_override(
                &peer_id,
                &address.ip().to_string(),
                Some(address.port()),
            ) {
                log::warn!("[cloud] failed to set peer addr override for {peer_id}: {error}");
            }
        }
        Ok(())
    }

    async fn publish_direct_endpoint(
        &self,
        record: &CloudMachineRecord,
        endpoint: &CloudEndpoint,
    ) -> Result<(), CloudError> {
        let installation_id = self.resolve_installation_id(record).ok_or_else(|| {
            CloudError::ProviderUnavailable(format!(
                "cloud machine {} has no installation id for endpoint publication",
                record.machine_id
            ))
        })?;
        self.state
            .mesh
            .lock()
            .pin_managed_cloud_endpoint(installation_id.clone(), endpoint.clone());
        #[cfg(any(test, feature = "test-support"))]
        if let Some(override_pub) = self.state.svc.try_ext::<CloudEndpointPublisherOverride>() {
            return (override_pub.0)(endpoint.clone(), installation_id)
                .await
                .map_err(CloudError::ProviderUnavailable);
        }
        // Registered-router fake-ingress tests install a connector override without a live TCP peer.
        #[cfg(any(test, feature = "test-support"))]
        if self
            .state
            .svc
            .try_ext::<crate::mesh_routes::relay::CloudIngressConnectorOverride>()
            .is_some()
        {
            return Ok(());
        }
        ferry_publish_direct_endpoint(&self.state.svc, endpoint.clone(), &installation_id)
            .await
            .map_err(|error| {
                CloudError::ProviderUnavailable(format!(
                    "publish_direct_endpoint failed: {error:?}"
                ))
            })
    }

    async fn await_peer_ready(
        &self,
        record: &CloudMachineRecord,
        deadline_ms: u64,
    ) -> Result<(), CloudError> {
        let mut rx = self.state.events.subscribe();
        let mut gate = self.state.mesh.lock().gate_watch();
        loop {
            if self.peer_is_online(record) {
                return Ok(());
            }
            let now_ms = self.state.svc.cloud_machines().now_ms();
            if now_ms >= deadline_ms {
                return Err(CloudError::WakeDeadlineExceeded {
                    machine_id: record.machine_id.clone(),
                });
            }
            let remaining = Duration::from_millis(deadline_ms.saturating_sub(now_ms));
            let wait = async {
                tokio::select! {
                    res = rx.recv() => match res {
                        Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => true,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => false,
                    },
                    res = gate.changed() => res.is_ok(),
                }
            };
            match tokio::time::timeout(remaining, wait).await {
                Ok(true) => {}
                Ok(false) | Err(_) => {
                    return Err(CloudError::WakeDeadlineExceeded {
                        machine_id: record.machine_id.clone(),
                    });
                }
            }
        }
    }

    async fn flush_send(
        &self,
        record: &CloudMachineRecord,
        send: &HeldCloudSend,
    ) -> FlushWriteOutcome {
        if send.body["require_agent"].as_bool() == Some(true) {
            let identify_token = match crate::mesh_routes::relay::sign_caller(
                &self.state,
                send.origin,
                send.agent.as_ref(),
            ) {
                Ok(token) => token,
                Err((_, detail)) => return FlushWriteOutcome::RefusedPreWrite(detail),
            };
            let identify_cmd = codeterm_service_core::mesh::types::RelayCommand {
                method: "GET".into(),
                path: "/api/identify".into(),
                body: serde_json::Value::Null,
                delivery_id: None,
            };
            let identify = if let Some(cloud_result) = self
                .send_cloud_relay_with_token(record, identify_cmd, identify_token.clone())
                .await
            {
                match cloud_result {
                    Ok(resp) if resp.ok => resp.data,
                    Ok(_) => None,
                    Err(crate::mesh_routes::relay::CloudIngressSendError::ConnectUnavailable(
                        _,
                    )) => return FlushWriteOutcome::RefusedMachineSleeping,
                    Err(_) => None,
                }
            } else {
                let Ok(client) = self.peer_client(record) else {
                    return FlushWriteOutcome::RefusedMachineSleeping;
                };
                match client
                    .with_signed_token(identify_token)
                    .auth_get_json("/api/identify")
                    .await
                {
                    Ok(value) => Some(value),
                    Err(_) => return FlushWriteOutcome::RefusedMachineSleeping,
                }
            };
            let supported = identify.as_ref().is_some_and(|id| {
                crate::mesh_routes::relay::identify_supports_feature(
                    id,
                    crate::mesh_routes::relay::REQUIRE_AGENT_SEND_FEATURE,
                )
            });
            if !supported {
                return FlushWriteOutcome::RefusedPreWrite("peer_lacks_agent_only_send".into());
            }
        }

        let token = match crate::mesh_routes::relay::sign_caller(
            &self.state,
            send.origin,
            send.agent.as_ref(),
        ) {
            Ok(token) => token,
            Err((_, detail)) => return FlushWriteOutcome::RefusedPreWrite(detail),
        };
        let cmd = codeterm_service_core::mesh::types::RelayCommand {
            method: send.method.clone(),
            path: send.path.clone(),
            body: send.body.clone(),
            delivery_id: Some(send.delivery_id.clone()),
        };
        let result = if let Some(cloud_result) = self
            .send_cloud_relay_with_token(record, cmd.clone(), token.clone())
            .await
        {
            match cloud_result {
                Ok(resp) => Ok(resp),
                Err(crate::mesh_routes::relay::CloudIngressSendError::ConnectUnavailable(_)) => {
                    return FlushWriteOutcome::RefusedMachineSleeping;
                }
                Err(crate::mesh_routes::relay::CloudIngressSendError::AdmissionRejected(
                    detail,
                )) => return FlushWriteOutcome::RefusedPreWrite(detail),
                Err(crate::mesh_routes::relay::CloudIngressSendError::Transport(error)) => {
                    Err((error.bytes_written(), error.to_string()))
                }
            }
        } else {
            let Ok(client) = self.peer_client(record) else {
                return FlushWriteOutcome::RefusedMachineSleeping;
            };
            client
                .with_signed_token(token)
                .send_relay(&cmd)
                .await
                .map_err(|error| (error.bytes_written(), error.to_string()))
        };
        match result {
            Ok(resp) if resp.ok => {
                let delivered = resp
                    .data
                    .as_ref()
                    .and_then(|d| d.get("delivery"))
                    .and_then(serde_json::Value::as_str)
                    == Some("sent");
                if delivered {
                    FlushWriteOutcome::Delivered
                } else {
                    FlushWriteOutcome::Written
                }
            }
            Ok(resp) => {
                let err_code = resp
                    .data
                    .as_ref()
                    .and_then(|d| d.pointer("/error/code"))
                    .and_then(serde_json::Value::as_str);
                let bytes_written = resp
                    .data
                    .as_ref()
                    .and_then(|d| d.pointer("/error/bytes_written"))
                    .and_then(serde_json::Value::as_bool);
                if err_code == Some("machine_sleeping") && bytes_written == Some(false) {
                    return FlushWriteOutcome::RefusedMachineSleeping;
                }
                if bytes_written == Some(false) {
                    return FlushWriteOutcome::RefusedPreWrite(
                        resp.error
                            .or_else(|| err_code.map(str::to_owned))
                            .unwrap_or_else(|| "refused_before_write".into()),
                    );
                }
                FlushWriteOutcome::Uncertain(
                    resp.error
                        .unwrap_or_else(|| format!("status {}", resp.status)),
                )
            }
            Err((bytes_written, detail)) => {
                if bytes_written == Some(false) {
                    FlushWriteOutcome::RefusedPreWrite(detail)
                } else {
                    FlushWriteOutcome::Uncertain(detail)
                }
            }
        }
    }

    async fn apply_warm_bootstrap(
        &self,
        record: &CloudMachineRecord,
        creation_generation: &str,
    ) -> Result<
        Option<codeterm_service_core::service::cloud_machines::warm_bootstrap::WarmBootstrapApplyResult>,
        CloudError,
    >{
        super::warm_bootstrap::relay_apply_warm_bootstrap(self, record, creation_generation).await
    }

    async fn prepare_provider_signin(
        &self,
        record: &CloudMachineRecord,
        request: &codeterm_service_core::service::cloud_machines::warm_bootstrap::PrepareProviderSignInRequest,
    ) -> Result<
        codeterm_service_core::service::cloud_machines::warm_bootstrap::PrepareProviderSignInResponse,
        CloudError,
    >{
        super::warm_bootstrap::relay_prepare_provider_signin(self, record, request).await
    }
}
