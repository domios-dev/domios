use super::*;
use codeterm_service_core::mesh::types::Permission;

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
        health_mode: codeterm_service_core::service::store::HealthMode::Http,
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
        read_only.capabilities[&InventoryOperation::UpdateSchedule].unavailable_reason,
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
        stale.capabilities[&InventoryOperation::UpdateSchedule].unavailable_reason,
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
        daemon.capabilities[&InventoryOperation::UpdateSchedule].unavailable_reason,
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
        cached.resources[0].capabilities[&InventoryOperation::Delete].unavailable_reason,
        Some(InventoryUnavailableReason::PartialPeerFailure)
    );
    assert!(healthy.resources[0].capabilities[&InventoryOperation::Delete].allowed);
}
