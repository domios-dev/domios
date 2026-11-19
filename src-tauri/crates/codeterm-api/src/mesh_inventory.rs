use std::collections::BTreeMap;

use axum::{extract::State as AxumState, http::HeaderMap, http::StatusCode, Json};

use crate::routes::AppState;
use codeterm_mesh::contract::inventory::{
    InventoryCapability, InventoryFreshness, InventoryOperation, InventoryProvenance,
    InventoryUnavailableReason,
};
use codeterm_service_core::api_support::access_policy::{PeerOperation, PeerResource, PeerScope};
use codeterm_service_core::mesh::types::AccessLevel;
use codeterm_service_core::mesh_host::{MeshInventoryResource, MeshInventoryView};
use codeterm_service_core::service::store::{Service, ServiceKind};

pub const INVENTORY_CONTRACT_VERSION: u16 = 1;

pub fn project_view(
    machine_id: impl Into<String>,
    machine_name: impl Into<String>,
    services: Vec<Service>,
    access: &AccessLevel,
    provenance: InventoryProvenance,
) -> MeshInventoryView {
    let machine_id = machine_id.into();
    let machine_name = machine_name.into();
    let resources = services
        .into_iter()
        .filter(|service| {
            operation_scope(&service.kind, &service.id, InventoryOperation::Get)
                .is_some_and(|scope| scope.allowed(access))
        })
        .map(|service| project_resource(&machine_id, &machine_name, service, access, &provenance))
        .collect();
    MeshInventoryView {
        contract_version: INVENTORY_CONTRACT_VERSION,
        machine_id,
        machine_name,
        resources,
        provenance,
    }
}

pub fn project_resource(
    machine_id: &str,
    machine_name: &str,
    service: Service,
    access: &AccessLevel,
    provenance: &InventoryProvenance,
) -> MeshInventoryResource {
    let resource_id = service.id.clone();
    MeshInventoryResource {
        resource_id,
        machine_id: machine_id.to_string(),
        machine_name: machine_name.to_string(),
        capabilities: capabilities_for_resource(
            access,
            &service.kind,
            &service.id,
            service.protected,
            provenance,
        ),
        provenance: provenance.clone(),
        service,
    }
}

pub fn capabilities_for_resource(
    access: &AccessLevel,
    kind: &ServiceKind,
    id: &str,
    protected: bool,
    provenance: &InventoryProvenance,
) -> BTreeMap<InventoryOperation, InventoryCapability> {
    InventoryOperation::all()
        .into_iter()
        .map(|operation| {
            (
                operation,
                capability_for_operation(access, kind, id, protected, provenance, operation),
            )
        })
        .collect()
}

fn capability_for_operation(
    access: &AccessLevel,
    kind: &ServiceKind,
    id: &str,
    protected: bool,
    provenance: &InventoryProvenance,
    operation: InventoryOperation,
) -> InventoryCapability {
    let Some(scope) = operation_scope(kind, id, operation) else {
        return InventoryCapability::unavailable(
            InventoryUnavailableReason::UnsupportedResourceKind,
        );
    };
    if !scope.allowed(access) {
        return InventoryCapability::unavailable(InventoryUnavailableReason::NotAuthorized);
    }
    if protected && operation == InventoryOperation::Delete {
        return InventoryCapability::unavailable(InventoryUnavailableReason::ProtectedResource);
    }
    if operation.is_mutating() {
        if provenance.partial_error.is_some() {
            return InventoryCapability::unavailable(
                InventoryUnavailableReason::PartialPeerFailure,
            );
        }
        match provenance.freshness {
            InventoryFreshness::Fresh => {}
            InventoryFreshness::Stale => {
                return InventoryCapability::unavailable(InventoryUnavailableReason::Stale);
            }
            InventoryFreshness::Offline => {
                return InventoryCapability::unavailable(InventoryUnavailableReason::Offline);
            }
        }
    }
    InventoryCapability::allowed()
}

fn operation_scope<'a>(
    kind: &ServiceKind,
    id: &'a str,
    operation: InventoryOperation,
) -> Option<PeerScope<'a>> {
    use InventoryOperation::*;
    use PeerOperation::*;
    let (operation, resource) = match (kind, operation) {
        (ServiceKind::Cron, Get | History) => (ReadCron, PeerResource::Cron(id)),
        (ServiceKind::Cron, UpdateSchedule | Delete | Enable | Disable | Trigger | Cancel) => {
            (ManageCron, PeerResource::Cron(id))
        }
        (ServiceKind::Daemon, Get) => (ReadService, PeerResource::Service(id)),
        (ServiceKind::Daemon, Delete) => (FullAccess, PeerResource::Service(id)),
        _ => return None,
    };
    Some(PeerScope::new(operation, resource))
}

pub async fn inventory(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<MeshInventoryView>, (StatusCode, String)> {
    tokio::task::spawn_blocking(move || {
        let access = crate::auth::caller_access_level(&state, &headers)?;
        let (machine_id, machine_name) = {
            let mesh = state.mesh.lock();
            (
                mesh.ident.identity.instance_id.clone(),
                mesh.ident.instance_name.clone(),
            )
        };
        let services = crate::services_routes::local_services_snapshot(&state)?;
        let provenance = InventoryProvenance {
            freshness: InventoryFreshness::Fresh,
            offline: false,
            fetched_at: Some(current_epoch_seconds()),
            partial_error: None,
        };
        Ok(Json(project_view(
            machine_id,
            machine_name,
            services,
            &access,
            provenance,
        )))
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

pub(crate) fn current_epoch_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests;
