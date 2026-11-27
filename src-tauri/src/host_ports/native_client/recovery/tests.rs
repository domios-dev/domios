use super::*;
use codeterm_types::server_bootstrap::{ClientIdentity, ServerComponentManifest};

#[test]
fn server_swap_preserves_client_and_protocol_identity() {
    let previous = ServerComponentManifest {
        schema: 2,
        server_build: "old".into(),
        server_executable: "domios-server/domios-server".into(),
        server_sha256: "a".repeat(64),
        client_identity: ClientIdentity::Sha256 {
            sha256: "b".repeat(64),
        },
        api_version: 3,
        bootstrap_protocol: 2,
    };
    let mut current = previous.clone();
    current.server_build = "new".into();
    current.server_sha256 = "c".repeat(64);
    assert!(same_client(&previous, &current));
    current.client_identity = ClientIdentity::Sha256 {
        sha256: "d".repeat(64),
    };
    assert!(!same_client(&previous, &current));
    current = previous.clone();
    current.bootstrap_protocol += 1;
    assert!(!same_client(&previous, &current));
    current = previous.clone();
    current.server_executable = "foreign/server".into();
    assert!(!same_client(&previous, &current));
}

fn fixture_ready(epoch: &str) -> codeterm_types::server_bootstrap::ServerReady {
    serde_json::from_value(serde_json::json!({
        "protocol": 2, "api_version": 3, "build": "opaque", "instance_id": "instance",
        "identity": {"data": "data", "config": "config"}, "epoch": epoch,
        "process": {"pid": 42, "created": "123", "executable": "server"},
        "api_address": "127.0.0.1:7887", "socket_address": "socket",
        "bootstrap_address": "bootstrap", "resources": "resources"
    }))
    .unwrap()
}

#[test]
fn stale_disconnect_cannot_clear_a_replacement_epoch() {
    let ready = fixture_ready("old");
    let slot = crate::host_ports::ServerApiAttachmentSlot::default();
    slot.install(ready.clone(), "opaque-token".into()).unwrap();
    let replacement = fixture_ready("new");
    slot.install(replacement.clone(), "opaque-token".into())
        .unwrap();
    assert!(!slot.clear_if_current(&ready));
    assert_eq!(slot.current().unwrap().ready(), &replacement);
    assert!(slot.clear_if_current(&replacement));
    assert!(slot.current().is_none());
}

#[test]
fn same_epoch_reattach_generation_blocks_stale_disconnect_from_wiping_api_attachment() {
    let ready = fixture_ready("same-epoch");
    let slot = crate::host_ports::ServerApiAttachmentSlot::default();
    let first = slot.install(ready.clone(), "token-1".into()).unwrap();
    let second = slot.install(ready.clone(), "token-2".into()).unwrap();
    assert!(second > first);
    assert!(!slot.matches_generation(first, &ready));
    assert!(!slot.clear_if_generation(first, &ready));
    let active = slot
        .current()
        .expect("same-epoch replacement stays attached");
    assert_eq!(active.token(), "token-2");
    assert!(slot.matches_generation(second, &ready));
    assert!(slot.clear_if_generation(second, &ready));
    assert!(slot.current().is_none());
}

#[test]
fn disconnect_recovery_state_machine_prefers_silent_reattach_for_live_server() {
    assert_eq!(
        decide_disconnect_recovery(DisconnectInput {
            was_current_client: false,
            api_generation_matches: true,
            client_exit_pending: false,
            server_process_alive: true,
            auto_reattach_allowed: true,
        }),
        DisconnectDecision::IgnoreStale
    );
    assert_eq!(
        decide_disconnect_recovery(DisconnectInput {
            was_current_client: true,
            api_generation_matches: false,
            client_exit_pending: false,
            server_process_alive: true,
            auto_reattach_allowed: true,
        }),
        DisconnectDecision::IgnoreStale
    );
    assert_eq!(
        decide_disconnect_recovery(DisconnectInput {
            was_current_client: true,
            api_generation_matches: true,
            client_exit_pending: true,
            server_process_alive: true,
            auto_reattach_allowed: true,
        }),
        DisconnectDecision::ClearForShutdown
    );
    assert_eq!(
        decide_disconnect_recovery(DisconnectInput {
            was_current_client: true,
            api_generation_matches: true,
            client_exit_pending: false,
            server_process_alive: true,
            auto_reattach_allowed: true,
        }),
        DisconnectDecision::SilentReattachThenEscalate
    );
    assert_eq!(
        decide_disconnect_recovery(DisconnectInput {
            was_current_client: true,
            api_generation_matches: true,
            client_exit_pending: false,
            server_process_alive: true,
            auto_reattach_allowed: false,
        }),
        DisconnectDecision::ClearAndEscalate
    );
    assert_eq!(
        decide_disconnect_recovery(DisconnectInput {
            was_current_client: true,
            api_generation_matches: true,
            client_exit_pending: false,
            server_process_alive: false,
            auto_reattach_allowed: true,
        }),
        DisconnectDecision::ClearAndEscalate
    );
}

#[test]
fn silent_reattach_budget_bounds_rapid_loops_and_recovers_after_window() {
    let mut budget = ReattachBudget::new();
    let start = Instant::now();
    assert!(budget.allow(start));
    assert!(budget.allow(start + Duration::from_secs(5)));
    assert!(budget.allow(start + Duration::from_secs(10)));
    assert!(!budget.allow(start + Duration::from_secs(15)));
    assert!(budget.allow(start + REATTACH_WINDOW));
    assert!(budget.allow(start + REATTACH_WINDOW + Duration::from_secs(5)));
}

#[test]
fn activation_reattach_triggers_only_when_api_is_attached_without_a_live_native_host() {
    assert!(should_reattach_on_activate(false, true, false));
    assert!(!should_reattach_on_activate(true, true, false));
    assert!(!should_reattach_on_activate(false, false, false));
    assert!(!should_reattach_on_activate(false, true, true));
}
