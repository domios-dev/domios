use super::tests::AnalyticsHost;
use crate::host_traits::PluginHostCtx;
use crate::js_runtime::host_api::PluginSandbox;
use crate::js_runtime::manifest::PermissionsConfig;
use crate::js_runtime::vm::{call_json, PluginVm};
use crate::local_recording::LocalRecordingTrigger;
use serde_json::{json, Value};
use std::sync::Arc;

fn recording_vm(host: &Arc<dyn PluginHostCtx>, granted: bool, id: &str) -> PluginVm {
    let sandbox = Arc::new(
        PluginSandbox::restricted(vec![], vec![]).with_identity_and_perms(
            id,
            &PermissionsConfig {
                local_recording: granted,
                ..Default::default()
            },
        ),
    );
    let vm = PluginVm::from_source_sandboxed(id, r#"module.exports.default = {
        read() { return host.localRecording.status(); },
        enable() { return host.localRecording.setEnabled(true); },
        disable() { return host.localRecording.setEnabled(false); },
        authority() { return [typeof host.localRecording.start, typeof host.localRecording.commit, typeof host.localRecording.selectDevices, typeof host.receiverTarget.set]; },
    }"#.into(), None, sandbox).unwrap();
    assert!(vm.set_service(Arc::downgrade(host)));
    vm
}

#[test]
fn lark_recording_contract_native_js_status_enable_revocation_and_confinement_parity() {
    let native = Arc::new(AnalyticsHost::default());
    native.recordings.register(
        "trigger",
        LocalRecordingTrigger {
            vendor_id: 1,
            product_id: 2,
            usage_page: 3,
            usage: 4,
            button: None,
        },
        true,
    );
    let host: Arc<dyn PluginHostCtx> = native.clone();
    let vm = recording_vm(&host, true, "trigger");
    let native_read: Value =
        serde_json::from_str(&native.local_recording_status_json("trigger")).unwrap();
    assert_eq!(call_json::<Value>(&vm, "read", &[]).unwrap(), native_read);
    let enabled: Value = call_json(&vm, "enable", &[]).unwrap();
    assert_eq!(
        enabled,
        serde_json::from_str::<Value>(&native.local_recording_status_json("trigger")).unwrap()
    );
    assert_eq!(enabled["captureActive"], false);
    assert_eq!(
        call_json::<Value>(&vm, "authority", &[]).unwrap(),
        json!(["undefined", "undefined", "function", "undefined"])
    );
    native.recordings.revoke("trigger");
    let refused: Value = call_json(&vm, "enable", &[]).unwrap();
    assert_eq!(
        refused,
        serde_json::from_str::<Value>(&native.local_recording_enabled_json("trigger", true))
            .unwrap()
    );
    assert_eq!(refused["error"], "local_recording_grant_required");
    assert_eq!(
        call_json::<Value>(&vm, "read", &[]).unwrap()["granted"],
        false
    );
    let foreign = recording_vm(&host, true, "foreign");
    assert_eq!(
        call_json::<Value>(&foreign, "read", &[]).unwrap()["error"],
        "trigger_not_declared"
    );
}

#[test]
fn lark_recording_contract_js_missing_grant_refuses_even_with_native_host_grant() {
    let native = Arc::new(AnalyticsHost::default());
    native.recordings.register(
        "trigger",
        LocalRecordingTrigger {
            vendor_id: 1,
            product_id: 2,
            usage_page: 3,
            usage: 4,
            button: None,
        },
        true,
    );
    let host: Arc<dyn PluginHostCtx> = native;
    let vm = recording_vm(&host, false, "trigger");
    assert_eq!(
        call_json::<Value>(&vm, "enable", &[]).unwrap()["error"],
        "local_recording_grant_required"
    );
    assert_eq!(
        call_json::<Value>(&vm, "read", &[]).unwrap()["enabled"],
        false
    );
}

#[test]
fn receiver_status_fixed_declaration_is_confined_and_native_js_equal() {
    let native = Arc::new(AnalyticsHost::default());
    let host: Arc<dyn PluginHostCtx> = native.clone();
    let query = crate::local_recording::ReceiverStatusQuery {
        usage_page: 9,
        usage: 1,
        report_id: 7,
        request: vec![7, 8],
    };
    let expected: Value =
        serde_json::from_str(&native.receiver_status_report_json("own", &query)).unwrap();
    for granted in [false, true] {
        let sandbox = Arc::new(
            PluginSandbox::restricted(vec![], vec![])
                .with_identity_and_perms(
                    "own",
                    &PermissionsConfig {
                        local_recording: granted,
                        ..Default::default()
                    },
                )
                .with_receiver_status_query(Some(query.clone())),
        );
        let vm = PluginVm::from_source_sandboxed("own", r#"module.exports.default = { read() { return host.localRecording.statusReport({request:[99],path:'foreign-device'}); } };"#.into(), None, sandbox).unwrap();
        assert!(vm.set_service(Arc::downgrade(&host)));
        let actual: Value = call_json(&vm, "read", &[]).unwrap();
        if granted {
            assert_eq!(actual, expected);
        } else {
            assert_eq!(actual["error"], "local_recording_grant_required");
        }
    }
}
#[test]
fn receiver_status_missing_or_oversized_declaration_refuses_io() {
    let host: Arc<dyn PluginHostCtx> = Arc::new(AnalyticsHost::default());
    for query in [
        None,
        Some(crate::local_recording::ReceiverStatusQuery {
            usage_page: 9,
            usage: 1,
            report_id: 7,
            request: vec![7; 65],
        }),
    ] {
        let sandbox = Arc::new(
            PluginSandbox::restricted(vec![], vec![])
                .with_identity_and_perms(
                    "own",
                    &PermissionsConfig {
                        local_recording: true,
                        ..Default::default()
                    },
                )
                .with_receiver_status_query(query),
        );
        let vm = PluginVm::from_source_sandboxed(
            "own",
            "module.exports.default = { read() { return host.localRecording.statusReport(); } };"
                .into(),
            None,
            sandbox,
        )
        .unwrap();
        assert!(vm.set_service(Arc::downgrade(&host)));
        assert_eq!(
            call_json::<Value>(&vm, "read", &[]).unwrap()["error"],
            "receiver_status_not_declared"
        );
    }
}
