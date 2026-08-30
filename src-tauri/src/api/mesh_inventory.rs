use std::collections::BTreeMap;

use axum::{extract::State as AxumState, http::HeaderMap, http::StatusCode, Json};
use serde::{Deserialize, Serialize};

use crate::api::routes::{check_auth, AppState};
use crate::mesh::types::AccessLevel;
use crate::service::store::{Service, ServiceKind};
use codeterm_types::mesh::inventory::{
    InventoryCapability, InventoryFreshness, InventoryOperation, InventoryProvenance,
    InventoryUnavailableReason,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshInventoryResource {
    #[serde(flatten)]
    pub service: Service,
    pub resource_id: String,
    pub machine_id: String,
    pub machine_name: String,
    pub provenance: InventoryProvenance,
    pub capabilities: BTreeMap<InventoryOperation, InventoryCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshInventoryView {
    pub contract_version: u16,
    pub machine_id: String,
    pub machine_name: String,
    pub resources: Vec<MeshInventoryResource>,
    pub provenance: InventoryProvenance,
}

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
    let Some((method, path)) = operation_route(kind, id, operation) else {
        return InventoryCapability::unavailable(InventoryUnavailableReason::UnsupportedResourceKind);
    };
    if !crate::api::access_policy::check_access(access, &method, &path) {
        return InventoryCapability::unavailable(InventoryUnavailableReason::NotAuthorized);
    }
    if protected && operation == InventoryOperation::Delete {
        return InventoryCapability::unavailable(InventoryUnavailableReason::ProtectedResource);
    }
    if operation.is_mutating() {
        if provenance.partial_error.is_some() {
            return InventoryCapability::unavailable(InventoryUnavailableReason::PartialPeerFailure);
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

fn operation_route(
    kind: &ServiceKind,
    id: &str,
    operation: InventoryOperation,
) -> Option<(axum::http::Method, String)> {
    let (prefix, supported) = match kind {
        ServiceKind::Cron => ("/api/crons", true),
        ServiceKind::Daemon => ("/api/services", matches!(operation, InventoryOperation::Get | InventoryOperation::Delete)),
    };
    if !supported {
        return None;
    }
    let (method, suffix) = match operation {
        InventoryOperation::Get => (axum::http::Method::GET, ""),
        InventoryOperation::History if matches!(kind, ServiceKind::Cron) => {
            (axum::http::Method::GET, "/history")
        }
        InventoryOperation::UpdateSchedule if matches!(kind, ServiceKind::Cron) => {
            (axum::http::Method::PUT, "")
        }
        InventoryOperation::Delete => (axum::http::Method::DELETE, ""),
        InventoryOperation::Enable if matches!(kind, ServiceKind::Cron) => {
            (axum::http::Method::POST, "/enable")
        }
        InventoryOperation::Disable if matches!(kind, ServiceKind::Cron) => {
            (axum::http::Method::POST, "/disable")
        }
        InventoryOperation::Trigger if matches!(kind, ServiceKind::Cron) => {
            (axum::http::Method::POST, "/trigger")
        }
        InventoryOperation::Cancel if matches!(kind, ServiceKind::Cron) => {
            (axum::http::Method::POST, "/cancel")
        }
        _ => return None,
    };
    Some((method, format!("{prefix}/{id}{suffix}")))
}

pub async fn inventory(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
) -> Result<Json<MeshInventoryView>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, error)| (status, error.to_string()))?;
    let access = crate::api::auth::caller_access_level(&state, &headers)?;
    let (machine_id, machine_name) = {
        let mesh = state.mesh.lock();
        (
            mesh.ident.identity.instance_id.clone(),
            mesh.ident.instance_name.clone(),
        )
    };
    let services = {
        let cron = state.cron.lock();
        crate::service::store::list_local(&state.svc.services_store, &cron)
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    };
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
}

pub(crate) fn current_epoch_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::types::Permission;

    fn service(kind: ServiceKind) -> Service {
        Service {
            id: "cron-1".into(),
            name: "job".into(),
            kind,
            protected: false,
            description: None,
            host: None,
            url: None,
            port: None,
            process_label: None,
            script_path: None,
            health_mode: crate::service::store::HealthMode::Http,
            health_path: None,
            health_json_field: None,
            health_ok_values: None,
            health_degraded_values: None,
            health_interval: 0,
            health_ttl: 0,
            health: Default::default(),
            field_mapping: None,
            push_enabled: false,
            schedule: Some("* * * * *".into()),
            next_run: None,
            last_run_status: None,
            registered_at: 0,
            updated_at: 0,
            enabled: true,
        }
    }

    fn provenance(freshness: InventoryFreshness) -> InventoryProvenance {
        InventoryProvenance {
            freshness,
            offline: freshness == InventoryFreshness::Offline,
            fetched_at: Some(1),
            partial_error: None,
        }
    }

    #[test]
    fn mesh_inventory_capability_projection_respects_access_and_resource_kind() {
        let full = project_resource(
            "machine-a",
            "Alpha",
            service(ServiceKind::Cron),
            &AccessLevel::Full,
            &provenance(InventoryFreshness::Fresh),
        );
        assert!(full.capabilities[&InventoryOperation::UpdateSchedule].allowed);
        assert!(full.capabilities[&InventoryOperation::Delete].allowed);
        for operation in InventoryOperation::all() {
            assert!(full.capabilities[&operation].allowed, "{operation:?}");
        }
        let capability_json = serde_json::to_value(&full.capabilities).unwrap();
        assert!(capability_json.get("start").is_none());
        assert!(capability_json.get("stop").is_none());

        let read_only = project_resource(
            "machine-b",
            "Beta",
            service(ServiceKind::Cron),
            &AccessLevel::Custom(vec![Permission::ReadTabs]),
            &provenance(InventoryFreshness::Fresh),
        );
        assert_eq!(
            read_only.capabilities[&InventoryOperation::UpdateSchedule]
                .unavailable_reason,
            Some(InventoryUnavailableReason::NotAuthorized)
        );
        assert!(read_only.capabilities[&InventoryOperation::Get].allowed);
        assert!(read_only.capabilities[&InventoryOperation::History].allowed);

        let stale = project_resource(
            "machine-b",
            "Beta",
            service(ServiceKind::Cron),
            &AccessLevel::Full,
            &provenance(InventoryFreshness::Stale),
        );
        assert_eq!(
            stale.capabilities[&InventoryOperation::UpdateSchedule]
                .unavailable_reason,
            Some(InventoryUnavailableReason::Stale)
        );
        let offline = project_resource(
            "machine-b",
            "Beta",
            service(ServiceKind::Cron),
            &AccessLevel::Full,
            &provenance(InventoryFreshness::Offline),
        );
        assert_eq!(
            offline.capabilities[&InventoryOperation::Delete].unavailable_reason,
            Some(InventoryUnavailableReason::Offline)
        );

        let daemon = project_resource(
            "machine-a",
            "Alpha",
            service(ServiceKind::Daemon),
            &AccessLevel::Full,
            &provenance(InventoryFreshness::Fresh),
        );
        assert_eq!(
            daemon.capabilities[&InventoryOperation::UpdateSchedule]
                .unavailable_reason,
            Some(InventoryUnavailableReason::UnsupportedResourceKind)
        );
        assert_eq!(
            daemon.capabilities[&InventoryOperation::Enable].unavailable_reason,
            Some(InventoryUnavailableReason::UnsupportedResourceKind)
        );
        assert!(daemon.capabilities[&InventoryOperation::Delete].allowed);

        let mut protected_cron = service(ServiceKind::Cron);
        protected_cron.protected = true;
        let protected_cron = project_resource(
            "machine-a",
            "Alpha",
            protected_cron,
            &AccessLevel::Full,
            &provenance(InventoryFreshness::Fresh),
        );
        assert_eq!(
            protected_cron.capabilities[&InventoryOperation::Delete].unavailable_reason,
            Some(InventoryUnavailableReason::ProtectedResource)
        );
        assert!(protected_cron.capabilities[&InventoryOperation::Disable].allowed);
    }

    #[test]
    fn mesh_inventory_partial_failure_retains_healthy_and_cached_rows() {
        let cached = project_view(
            "machine-b",
            "Beta",
            vec![service(ServiceKind::Cron)],
            &AccessLevel::Full,
            InventoryProvenance {
                freshness: InventoryFreshness::Stale,
                offline: true,
                fetched_at: Some(1),
                partial_error: Some("peer unavailable".into()),
            },
        );
        let healthy = project_view(
            "machine-a",
            "Alpha",
            vec![service(ServiceKind::Daemon)],
            &AccessLevel::Full,
            provenance(InventoryFreshness::Fresh),
        );
        assert_eq!(cached.resources.len() + healthy.resources.len(), 2);
        assert_eq!(cached.resources[0].machine_id, "machine-b");
        assert_eq!(
            cached.resources[0].capabilities[&InventoryOperation::Delete]
                .unavailable_reason,
            Some(InventoryUnavailableReason::PartialPeerFailure)
        );
        assert!(healthy.resources[0].capabilities[&InventoryOperation::Delete].allowed);
    }
}
