use super::*;
use codeterm_account::account_mesh::wire::{
    Eligibility, MeshEndpointObservation, PublicationState,
};
use codeterm_service_core::service::cloud_machines::{
    CloudLifecycleState, CloudMachineRecord, CloudOwnershipReceipt, CloudSleepMode,
    CloudStorageTier, AUX_KEY_STORAGE_TIER,
};
use serde::Deserialize;

const NOW: u64 = 1_800_000_000;
const SHARED_STALE_FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../tests/fixtures/mesh-stale-cleanup.json"
));

fn row(
    id: &str,
    name: Option<&str>,
    authenticated_at: u64,
    active: bool,
    presence: RegistryPresence,
    membership_expires_at: Option<u64>,
    endpoint_expires_at: Option<u64>,
) -> MeshManagementRow {
    MeshManagementRow {
        installation_id: id.into(),
        key_id: "a".repeat(64),
        key_generation: 1,
        slot_revision: 1,
        active,
        presence,
        admitted_at: authenticated_at,
        authenticated_at,
        display_name: name.map(str::to_owned),
        metadata_revision: 1,
        publication: PublicationState::Complete,
        membership_expires_at,
        eligibility: if active {
            Eligibility::Eligible
        } else {
            Eligibility::Revoked
        },
        endpoint: endpoint_expires_at.map(|expires_at| MeshEndpointObservation {
            transport: None,
            tailnet_ip: "100.64.0.10".into(),
            api_port: 7685,
            expires_at,
        }),
    }
}

#[test]
fn default_selection_picks_expired_signed_out_and_long_unreachable_and_spares_self_online_recent() {
    let rows = vec![
        // Self: must never be selected even if expired.
        row(
            "self-id",
            Some("My-PC"),
            NOW - 30 * SECONDS_PER_DAY,
            true,
            RegistryPresence::Present,
            Some(NOW - 10),
            None,
        ),
        // Already revoked: ignored.
        row(
            "revoked-id",
            Some("Old-Revoked"),
            NOW - 30 * SECONDS_PER_DAY,
            false,
            RegistryPresence::Present,
            Some(NOW - 100),
            None,
        ),
        // Online by unexpired membership + endpoint: spared.
        row(
            "online-registry",
            Some("Workstation"),
            NOW - 100,
            true,
            RegistryPresence::Present,
            Some(NOW + 3600),
            Some(NOW + 1800),
        ),
        // Live-connected peer even with stale endpoint metadata: spared.
        row(
            "online-live",
            Some("Cloud-Box"),
            NOW - 20 * SECONDS_PER_DAY,
            true,
            RegistryPresence::Present,
            Some(NOW - 10),
            None,
        ),
        // Recently signed out: selected under default mode.
        row(
            "signed-out-recent",
            Some("Laptop"),
            NOW - 3600,
            true,
            RegistryPresence::SignedOut,
            Some(NOW + 3600),
            None,
        ),
        // Expired membership: selected under default mode.
        row(
            "expired-recent",
            Some("Tablet"),
            NOW - 7200,
            true,
            RegistryPresence::Present,
            Some(NOW - 60),
            None,
        ),
        // Unreachable for 2 days (< 7d default): spared.
        row(
            "unreachable-recent",
            Some("Brief-Offline"),
            NOW - 2 * SECONDS_PER_DAY,
            true,
            RegistryPresence::Present,
            Some(NOW + 3600),
            Some(NOW - 2 * SECONDS_PER_DAY),
        ),
        // Unreachable for 10 days (>= 7d default): selected.
        row(
            "unreachable-old",
            Some("Abandoned"),
            NOW - 10 * SECONDS_PER_DAY,
            true,
            RegistryPresence::Present,
            None,
            None,
        ),
    ];
    let live: HashSet<String> = ["online-live".to_owned()].into_iter().collect();
    let protected_cloud = HashSet::new();

    let selected = select_stale_installations(&rows, "self-id", &live, &protected_cloud, NOW, None);
    let summary: Vec<(&str, StaleReason)> = selected
        .iter()
        .map(|c| (c.row.installation_id.as_str(), c.reason))
        .collect();
    assert_eq!(
        summary,
        vec![
            ("unreachable-old", StaleReason::Unreachable),
            ("signed-out-recent", StaleReason::SignedOut),
            ("expired-recent", StaleReason::MembershipExpired),
        ]
    );
}

#[test]
fn older_than_days_filters_all_stale_categories_by_age_and_never_selects_online_or_self() {
    let rows = vec![
        row(
            "self-id",
            Some("Self"),
            NOW - 60 * SECONDS_PER_DAY,
            true,
            RegistryPresence::SignedOut,
            None,
            None,
        ),
        row(
            "online-id",
            Some("Online"),
            NOW - 60 * SECONDS_PER_DAY,
            true,
            RegistryPresence::Present,
            Some(NOW + 3600),
            Some(NOW + 3600),
        ),
        row(
            "signed-out-2d",
            Some("Recent-Out"),
            NOW - 2 * SECONDS_PER_DAY,
            true,
            RegistryPresence::SignedOut,
            None,
            None,
        ),
        row(
            "expired-30d",
            Some("Old-Expired"),
            NOW - 35 * SECONDS_PER_DAY,
            true,
            RegistryPresence::Present,
            Some(NOW - 30 * SECONDS_PER_DAY),
            None,
        ),
        row(
            "unreachable-2d",
            Some("Offline-2d"),
            NOW - 2 * SECONDS_PER_DAY,
            true,
            RegistryPresence::Present,
            Some(NOW + 3600),
            Some(NOW - 2 * SECONDS_PER_DAY),
        ),
    ];
    let live = HashSet::new();
    let protected_cloud = HashSet::new();

    // --older-than 14 only keeps the 30-day-old expired machine.
    let older_14 =
        select_stale_installations(&rows, "self-id", &live, &protected_cloud, NOW, Some(14));
    assert_eq!(
        older_14
            .iter()
            .map(|c| c.row.installation_id.as_str())
            .collect::<Vec<_>>(),
        vec!["expired-30d"]
    );

    // --older-than 0 includes every non-online stale machine, still sparing self and online.
    let older_0 =
        select_stale_installations(&rows, "self-id", &live, &protected_cloud, NOW, Some(0));
    assert_eq!(
        older_0
            .iter()
            .map(|c| c.row.installation_id.as_str())
            .collect::<Vec<_>>(),
        vec!["expired-30d", "signed-out-2d", "unreachable-2d"]
    );
}

#[derive(Deserialize)]
struct SharedStaleFixture {
    now: u64,
    local_installation_id: String,
    cloud_machines: Vec<SharedCloudFixture>,
    pair_bindings: Vec<(String, String)>,
    registry_rows: Vec<SharedRegistryRowFixture>,
    expected_selected: Vec<SharedExpectedSelected>,
    expected_excluded_cloud_ids: Vec<String>,
}

#[derive(Deserialize)]
struct SharedCloudFixture {
    machine_id: String,
    name: String,
    provider: String,
    size: String,
    state: CloudLifecycleState,
    sleep_mode: CloudSleepMode,
    installation_id: Option<String>,
    peer_instance_id: Option<String>,
    storage_tier: Option<CloudStorageTier>,
}

#[derive(Deserialize)]
struct SharedRegistryRowFixture {
    installation_id: String,
    display_name: Option<String>,
    active: bool,
    presence: RegistryPresence,
    authenticated_at: u64,
    membership_expires_at: Option<u64>,
    endpoint_expires_at: Option<u64>,
    online: bool,
}

#[derive(Deserialize)]
struct SharedExpectedSelected {
    installation_id: String,
    display_name: String,
    reason: String,
}

fn fixture_cloud_record(entry: &SharedCloudFixture) -> CloudMachineRecord {
    let mut ownership = CloudOwnershipReceipt::default();
    if let Some(tier) = entry.storage_tier {
        ownership
            .auxiliary_resource_ids
            .insert(AUX_KEY_STORAGE_TIER.into(), tier.as_str().into());
    }
    CloudMachineRecord {
        machine_id: entry.machine_id.clone(),
        name: entry.name.clone(),
        provider: entry.provider.clone(),
        size: entry.size.clone(),
        state: entry.state,
        sleep_mode: entry.sleep_mode,
        installation_id: entry.installation_id.clone(),
        peer_instance_id: entry.peer_instance_id.clone(),
        endpoint: None,
        rate_per_hour: None,
        created_at_ms: 0,
        last_awake_at_ms: None,
        last_owner_interaction_at_ms: None,
        accumulated_awake_secs: 0,
        period_start_ms: 0,
        lease_id: None,
        lease_expires_at_ms: None,
        exhaustion_pending: false,
        idle_since_ms: None,
        apply_unconfirmed: false,
        pending_request: None,
        last_error: None,
        ownership,
    }
}

fn parse_reason(raw: &str) -> StaleReason {
    match raw {
        "signed_out" => StaleReason::SignedOut,
        "membership_expired" => StaleReason::MembershipExpired,
        "unreachable" => StaleReason::Unreachable,
        other => panic!("unexpected fixture stale reason: {other}"),
    }
}

#[test]
fn stale_selection_excludes_live_cloud_machines_across_states_on_shared_fixture() {
    let fixture: SharedStaleFixture =
        serde_json::from_str(SHARED_STALE_FIXTURE).expect("parse shared stale cleanup fixture");

    let cloud_records: Vec<CloudMachineRecord> = fixture
        .cloud_machines
        .iter()
        .map(fixture_cloud_record)
        .collect();
    let protected_cloud = protected_cloud_installation_ids(&cloud_records, &fixture.pair_bindings);

    let rows: Vec<MeshManagementRow> = fixture
        .registry_rows
        .iter()
        .map(|entry| {
            row(
                &entry.installation_id,
                entry.display_name.as_deref(),
                entry.authenticated_at,
                entry.active,
                entry.presence,
                entry.membership_expires_at,
                entry.endpoint_expires_at,
            )
        })
        .collect();
    let live: HashSet<String> = fixture
        .registry_rows
        .iter()
        .filter(|entry| entry.online)
        .map(|entry| entry.installation_id.clone())
        .collect();

    let selection = select_stale_installations_with_cloud_exclusions(
        &rows,
        &fixture.local_installation_id,
        &live,
        &protected_cloud,
        fixture.now,
        None,
    );

    let actual_selected: Vec<(&str, &str, StaleReason)> = selection
        .candidates
        .iter()
        .map(|c| (c.row.installation_id.as_str(), c.label.as_str(), c.reason))
        .collect();
    let expected_selected: Vec<(&str, &str, StaleReason)> = fixture
        .expected_selected
        .iter()
        .map(|e| {
            (
                e.installation_id.as_str(),
                e.display_name.as_str(),
                parse_reason(&e.reason),
            )
        })
        .collect();
    assert_eq!(actual_selected, expected_selected);
    assert_eq!(
        selection.excluded_cloud_ids,
        fixture.expected_excluded_cloud_ids
    );
}

#[test]
fn batch_remove_skipped_cloud_machine_decodes_and_maps_to_refused_cloud() {
    let raw = r#"{
        "schema_version": 1,
        "registry_revision": 7,
        "removed_count": 1,
        "already_removed_count": 0,
        "skipped_count": 1,
        "results": [
            {
                "installation_id": "880cbbeedc6f433f8f332c9af20bc8c8",
                "status": "skipped",
                "slot_revision": 2,
                "reason": "cloud_machine"
            },
            {
                "installation_id": "inst-ordinary-stale",
                "status": "removed",
                "slot_revision": 4
            }
        ]
    }"#;
    let decoded: codeterm_account::account_mesh::wire::BatchRemoveResult =
        serde_json::from_str(raw).expect("decode BatchRemoveResult with skipped cloud_machine");
    assert_eq!(decoded.results.len(), 2);

    let cloud_row = &decoded.results[0];
    assert_eq!(cloud_row.status, BatchRemoveRowStatus::Skipped);
    assert_eq!(cloud_row.reason.as_deref(), Some("cloud_machine"));
    let mapped_cloud =
        RemoveStaleRowStatus::from_batch_row(cloud_row.status, cloud_row.reason.as_deref());
    assert_eq!(mapped_cloud, RemoveStaleRowStatus::RefusedCloud);
    assert_eq!(
        serde_json::to_value(mapped_cloud).unwrap(),
        serde_json::json!("refused_cloud")
    );

    let removed_row = &decoded.results[1];
    assert_eq!(removed_row.status, BatchRemoveRowStatus::Removed);
    assert_eq!(removed_row.reason, None);
    assert_eq!(
        RemoveStaleRowStatus::from_batch_row(removed_row.status, removed_row.reason.as_deref()),
        RemoveStaleRowStatus::Removed
    );

    assert_eq!(
        RemoveStaleRowStatus::from_batch_row(BatchRemoveRowStatus::Skipped, Some("other_guard")),
        RemoveStaleRowStatus::Skipped
    );
}
