use std::collections::BTreeMap;
use std::sync::Arc;

use super::super::ports::Clock;
use super::super::store::CloudMachineStore;
use super::super::types::{
    CloudLifecycleState, CloudMachineRecord, CloudOwnershipReceipt, CloudSleepMode,
};
use super::*;

struct FixedClock(u64);
impl Clock for FixedClock {
    fn now_ms(&self) -> u64 {
        self.0
    }
}

fn cloud_record(
    machine_id: &str,
    name: &str,
    installation_id: Option<&str>,
    peer_instance_id: Option<&str>,
) -> CloudMachineRecord {
    CloudMachineRecord {
        machine_id: machine_id.into(),
        name: name.into(),
        provider: "fly_machines".into(),
        size: "small".into(),
        state: CloudLifecycleState::Awake,
        sleep_mode: CloudSleepMode::Stop,
        installation_id: installation_id.map(str::to_owned),
        peer_instance_id: peer_instance_id.map(str::to_owned),
        endpoint: None,
        rate_per_hour: None,
        created_at_ms: 1_000,
        last_awake_at_ms: Some(1_000),
        last_owner_interaction_at_ms: Some(1_000),
        accumulated_awake_secs: 0,
        period_start_ms: 1_000,
        lease_id: None,
        lease_expires_at_ms: None,
        exhaustion_pending: false,
        idle_since_ms: None,
        apply_unconfirmed: false,
        pending_request: None,
        last_error: None,
        ownership: CloudOwnershipReceipt {
            provider_machine_id: "fly-1".into(),
            floating_ip_id: None,
            auxiliary_resource_ids: BTreeMap::new(),
            controller_id: "ctrl-1".into(),
            labels: BTreeMap::new(),
        },
    }
}

#[test]
fn enrich_peers_json_matches_by_peer_instance_id_and_installation_binding_and_applies_cloud_name() {
    let inst_id = "e45c9e4ff42143f0bee45f1f75b11a9c";
    let peer_id = "3efa6b47-cbad-4243-b381-86e57930319a";

    // Case 1: record already has peer_instance_id.
    let with_peer = cloud_record("cloud-1", "quill-fly-124-go", Some(inst_id), Some(peer_id));
    let raw = serde_json::json!({
        "paired": [{
            "instance_id": peer_id,
            "instance_name": peer_id,
            "online": true,
            "addresses": [],
            "api_port": 0,
            "relay_protocol_version": 3,
        }],
        "installations": [{
            "installation_id": inst_id,
            "instance_id": peer_id,
            "name": "",
            "active": true,
            "online": true,
        }],
    });
    let enriched = enrich_peers_json(
        raw.clone(),
        std::slice::from_ref(&with_peer),
        &[with_peer.receipt(61_000, None)],
        61_000,
    );
    assert_eq!(enriched["paired"][0]["kind"], "cloud");
    assert_eq!(enriched["paired"][0]["instance_name"], "quill-fly-124-go");
    assert_eq!(enriched["paired"][0]["cloud"]["machine_id"], "cloud-1");
    assert_eq!(enriched["paired"][0]["cloud"]["name"], "quill-fly-124-go");
    assert_eq!(enriched["installations"][0]["kind"], "cloud");
    assert_eq!(enriched["installations"][0]["name"], "quill-fly-124-go");

    // Case 2: legacy record missing peer_instance_id still matches via installations binding.
    let legacy = cloud_record("cloud-1", "quill-fly-124-go", Some(inst_id), None);
    let enriched_legacy = enrich_peers_json(
        raw,
        std::slice::from_ref(&legacy),
        &[legacy.receipt(61_000, None)],
        61_000,
    );
    assert_eq!(enriched_legacy["paired"][0]["kind"], "cloud");
    assert_eq!(
        enriched_legacy["paired"][0]["instance_name"],
        "quill-fly-124-go"
    );
    assert_eq!(
        enriched_legacy["paired"][0]["cloud"]["machine_id"],
        "cloud-1"
    );
}

#[test]
fn enrich_peers_value_backfills_and_persists_peer_instance_id_for_existing_records() {
    let dir = tempfile::tempdir().unwrap();
    let store_path = dir.path().join("cloud_machines.json");
    let inst_id = "e45c9e4ff42143f0bee45f1f75b11a9c";
    let peer_id = "3efa6b47-cbad-4243-b381-86e57930319a";
    let store = CloudMachineStore::new(store_path.clone());
    store
        .save(&[cloud_record(
            "cloud-DPGKwwYH",
            "quill-fly-124-go",
            Some(inst_id),
            None,
        )])
        .unwrap();

    let ctrl = CloudMachineController::new(store, "ctrl-1".into(), Arc::new(FixedClock(10_000)));
    assert_eq!(ctrl.list()[0].peer_instance_id, None);

    let raw = serde_json::json!({
        "paired": [{
            "instance_id": peer_id,
            "instance_name": peer_id,
            "online": true,
        }],
        "installations": [{
            "installation_id": inst_id,
            "instance_id": peer_id,
            "name": "",
            "active": true,
            "online": true,
        }],
    });
    let enriched = ctrl.enrich_peers_value(raw);
    assert_eq!(enriched["paired"][0]["kind"], "cloud");
    assert_eq!(enriched["paired"][0]["instance_name"], "quill-fly-124-go");
    assert_eq!(enriched["cloud_machines"][0]["peer_instance_id"], peer_id);

    // Reloading from disk proves the backfilled peer_instance_id was persisted.
    let reloaded = CloudMachineStore::new(store_path).load();
    assert_eq!(reloaded[0].peer_instance_id.as_deref(), Some(peer_id));
}
