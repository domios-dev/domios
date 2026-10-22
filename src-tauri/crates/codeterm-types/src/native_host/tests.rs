use super::*;
use serde_json::json;

#[test]
fn enrollment_does_not_accept_peer_process_claims() {
    let request = json!({
        "protocol": NATIVE_HOST_PROTOCOL,
        "challenge": "challenge",
        "connection": {
            "instance_id": "instance",
            "identity": { "data": "data", "config": "config" },
            "server_epoch": "epoch",
            "generation": 1
        },
        "view": null,
        "client_process": {
            "pid": 12,
            "created": "token",
            "executable": "client.exe"
        }
    });

    assert!(serde_json::from_value::<NativeHostEnrollmentRequest>(request).is_err());
}

#[test]
fn native_operation_is_a_finite_typed_enum() {
    let operation = NativeHostOperation::<String>::NativeUpdate {
        request: "transaction".into(),
        apply: None,
    };
    let value = serde_json::to_value(&operation).unwrap();

    assert_eq!(value["operation"], "native_update");
    assert_eq!(value["payload"]["request"], "transaction");
    let observation = NativeHostOperation::<String>::ObserveNativeUpdate {
        operation_id: crate::native_update::NativeUpdateOperationId::new(Uuid::new_v4()).unwrap(),
        after_sequence: 8,
    };
    let value = serde_json::to_value(observation).unwrap();
    assert_eq!(value["operation"], "observe_native_update");
    assert_eq!(value["payload"]["after_sequence"], 8);
    assert!(
        serde_json::from_value::<NativeHostOperation<String>>(json!({
            "operation": "invoke",
            "payload": { "name": "focus_window" }
        }))
        .is_err()
    );
}

#[test]
fn observation_outcomes_round_trip_through_the_native_host_frame() {
    let connection = NativeHostConnection {
        instance_id: "instance".into(),
        identity: crate::server_bootstrap::ServerIdentity {
            data: "data".into(),
            config: "config".into(),
        },
        server_epoch: "epoch".into(),
        generation: 3,
    };
    let update = crate::native_update::NativeUpdateResult {
        operation_id: crate::native_update::NativeUpdateOperationId::new(Uuid::new_v4()).unwrap(),
        sequence: 4,
        connection: connection.clone(),
        state: crate::native_update::NativeUpdateState::Uncertain {
            reason: crate::native_update::NativeUpdateUncertainty::DisconnectedAfterDispatch,
        },
    };
    let outcomes = [
        crate::native_update::NativeUpdateObservationOutcome::Reply(update),
        crate::native_update::NativeUpdateObservationOutcome::NoNewEvidence,
        crate::native_update::NativeUpdateObservationOutcome::Unavailable {
            reason: crate::native_update::NativeUpdateUncertainty::DeadlineAfterDispatch,
        },
    ];

    for outcome in outcomes {
        let frame = NativeHostFrame::<String, crate::native_update::NativeUpdateResult>::Reply(
            NativeHostReply {
                request_id: NativeHostRequestId(17),
                connection: connection.clone(),
                scope: NativeHostScope::Client,
                completion: NativeHostCompletion::Completed(
                    NativeHostResult::NativeUpdateObservation(outcome),
                ),
            },
        );
        let encoded = serde_json::to_vec(&frame).unwrap();
        let decoded: NativeHostFrame<String, crate::native_update::NativeUpdateResult> =
            serde_json::from_slice(&encoded).unwrap();
        assert_eq!(decoded, frame);
    }
}

#[test]
fn diagnostics_status_frame_carries_only_the_bound_client_view_claim() {
    let report = NativeWebviewDiagnosticsReport {
        connection: NativeHostConnection {
            instance_id: "instance".into(),
            identity: crate::server_bootstrap::ServerIdentity {
                data: "data".into(),
                config: "config".into(),
            },
            server_epoch: "epoch".into(),
            generation: 3,
        },
        view_session: Uuid::new_v4(),
        status: NativeWebviewDiagnosticsStatus::Active {
            port: 9222,
            view_generation: 4,
        },
    };
    let frame = NativeHostFrame::<String, String>::DiagnosticsStatus(report);
    let value = serde_json::to_value(&frame).unwrap();

    assert_eq!(value["kind"], "diagnostics_status");
    assert_eq!(value["payload"]["status"]["status"], "active");
    assert_eq!(value["payload"]["status"]["payload"]["port"], 9222);
    assert!(value["payload"].get("client_process").is_none());
    assert!(value["payload"].get("origin").is_none());
    assert_eq!(
        serde_json::from_value::<NativeHostFrame<String, String>>(value).unwrap(),
        frame
    );
}

#[test]
fn diagnostics_active_status_rejects_reserved_ports_and_zero_view_generation() {
    assert!(!NativeWebviewDiagnosticsStatus::Active {
        port: 1023,
        view_generation: 1,
    }
    .is_well_formed());
    assert!(!NativeWebviewDiagnosticsStatus::Active {
        port: 9222,
        view_generation: 0,
    }
    .is_well_formed());
    assert!(NativeWebviewDiagnosticsStatus::Active {
        port: u16::MAX,
        view_generation: 1,
    }
    .is_well_formed());
}

#[test]
fn native_event_keeps_its_audience_explicit() {
    let event = NativeHostEvent {
        event_id: 7,
        connection: NativeHostConnection {
            instance_id: "instance".into(),
            identity: crate::server_bootstrap::ServerIdentity {
                data: "data".into(),
                config: "config".into(),
            },
            server_epoch: "epoch".into(),
            generation: 3,
        },
        audience: NativeHostEventAudience::Targeted("main".into()),
        name: "selection-changed".into(),
        payload: json!({ "selection": "tab" }),
    };
    let value = serde_json::to_value(&event).unwrap();

    assert_eq!(
        value["audience"],
        json!({ "audience": "targeted", "target": "main" })
    );
    assert_eq!(value["payload"], json!({ "selection": "tab" }));
    assert_eq!(
        serde_json::from_value::<NativeHostEvent>(value).unwrap(),
        event
    );
}

#[test]
fn native_request_does_not_accept_caller_origin_claims() {
    let request = json!({
        "request_id": 1,
        "connection": {
            "instance_id": "instance",
            "identity": { "data": "data", "config": "config" },
            "server_epoch": "epoch",
            "generation": 1
        },
        "scope": { "scope": "client" },
        "operation": {
            "operation": "window_exists",
            "payload": { "label": "main" }
        },
        "origin": "https://untrusted.example"
    });

    assert!(serde_json::from_value::<NativeHostRequest<String>>(request).is_err());
}

#[test]
fn dispatched_disconnect_is_uncertain_not_rollback() {
    let completion: NativeHostCompletion<()> =
        NativeHostCompletion::Uncertain(NativeHostUncertainty::DisconnectedAfterDispatch);
    let value = serde_json::to_value(completion).unwrap();

    assert_eq!(value["completion"], "uncertain");
    assert_eq!(value["payload"], "disconnected_after_dispatch");
    assert!(value.get("rolled_back").is_none());
}

#[test]
fn view_binding_requires_an_origin_and_non_nil_session() {
    let valid = NativeViewBinding::observed("tauri://localhost".into()).unwrap();
    let invalid = NativeViewBinding {
        origin: String::new(),
        session_id: Uuid::nil(),
    };

    assert!(valid.is_well_formed());
    assert!(!invalid.is_well_formed());
}

#[test]
fn native_host_socket_address_reuses_the_exact_bootstrap_identity() {
    let identity = "a".repeat(BOOTSTRAP_IDENTITY_HEX_LEN);
    assert_eq!(
        native_host_socket_address(&format!("domios-bootstrap-{identity}.sock")),
        Some(format!("domios-native-host-{identity}.sock"))
    );
    assert_eq!(
        native_host_socket_address("domios-bootstrap-not-a-digest.sock"),
        None
    );
    assert_eq!(
        native_host_socket_address(&format!("domios-bootstrap-{}.sock", "a".repeat(64))),
        None
    );
}

#[test]
fn local_socket_names_fit_sun_path_under_a_long_macos_tmpdir() {
    let macos_tmpdir = "/var/folders/zz/zyxvpxvq6csfxvn_n0000000000000/T/";
    let bootstrap = format!(
        "domios-bootstrap-{}.sock",
        "f".repeat(BOOTSTRAP_IDENTITY_HEX_LEN)
    );
    let native_host = native_host_socket_address(&bootstrap).expect("derived");
    for name in [bootstrap, native_host] {
        assert!(
            macos_tmpdir.len() + name.len() < 104,
            "{name} overflows sun_path"
        );
    }
}
