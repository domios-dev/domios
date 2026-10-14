use codeterm_types::local_recording::LocalRecordingTrigger;
use std::sync::Arc;

use serde_json::{json, Value};

use crate::js_runtime::host_compute::tests::AnalyticsHost;
use crate::js_runtime::{
    host_api::PluginSandbox,
    manifest::PermissionsConfig,
    vm::{call_json, PluginVm},
};
use crate::{host_traits::PluginHostCtx, local_recording::SelectedDevices};

fn devices() -> SelectedDevices {
    SelectedDevices {
        input_id: "input".into(),
        collection_id: "collection".into(),
        microphone_id: "microphone".into(),
    }
}

fn fixture(granted: bool, id: &str) -> (Arc<AnalyticsHost>, PluginVm, Arc<dyn PluginHostCtx>) {
    let native = Arc::new(AnalyticsHost::default());
    native.recordings.register(
        "own",
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
        list() { return host.localRecording.listInputDevices('foreign'); },
        saved() { return host.localRecording.savedSelection('foreign'); },
        select(selection) { return host.localRecording.selectDevices(selection); },
        authority() { return [typeof host.localRecording.start, typeof host.localRecording.commit]; },
    };"#.into(), None, sandbox).unwrap();
    assert!(vm.set_service(Arc::downgrade(&host)));
    (native, vm, host)
}

#[test]
fn local_recording_devices_native_js_parity_for_inventory_selection_saved_identity_and_revocation()
{
    let (native, vm, _host) = fixture(true, "own");
    let inputs: Value = serde_json::from_str(&native.local_recording_inputs_json("own")).unwrap();
    assert_eq!(call_json::<Value>(&vm, "list", &[]).unwrap(), inputs);
    assert_eq!(call_json::<Value>(&vm, "saved", &[]).unwrap(), Value::Null);
    let selection = devices();
    let generation: Value = call_json(&vm, "select", &[json!(selection)]).unwrap();
    assert_eq!(
        generation,
        json!(native.recordings.status("own").unwrap().generation)
    );
    assert_eq!(
        call_json::<Value>(&vm, "saved", &[]).unwrap(),
        json!(selection)
    );
    assert_eq!(
        call_json::<Value>(&vm, "saved", &[]).unwrap(),
        serde_json::from_str::<Value>(&native.local_recording_saved_selection_json("own")).unwrap()
    );
    let native_generation: Value =
        serde_json::from_str(&native.local_recording_select_json("own", selection.clone()))
            .unwrap();
    assert!(native_generation.as_u64().unwrap() > generation.as_u64().unwrap());
    assert_eq!(
        call_json::<Value>(&vm, "saved", &[]).unwrap(),
        json!(selection)
    );
    assert_eq!(
        call_json::<Value>(&vm, "authority", &[]).unwrap(),
        json!(["undefined", "undefined"])
    );
    native.recordings.revoke("own");
    for method in ["list", "saved"] {
        assert_eq!(
            call_json::<Value>(&vm, method, &[]).unwrap()["error"],
            "local_recording_grant_required"
        );
    }
    let refused: Value = call_json(&vm, "select", &[json!(selection)]).unwrap();
    assert_eq!(
        refused,
        serde_json::from_str::<Value>(&native.local_recording_select_json("own", selection))
            .unwrap()
    );
}

#[test]
fn local_recording_devices_missing_permission_foreign_identity_and_invalid_selection_refuse() {
    for (granted, id, error) in [
        (false, "own", "local_recording_grant_required"),
        (true, "foreign", "trigger_not_declared"),
    ] {
        let (_native, vm, _host) = fixture(granted, id);
        for method in ["list", "saved"] {
            assert_eq!(
                call_json::<Value>(&vm, method, &[]).unwrap()["error"],
                error
            );
        }
        assert_eq!(
            call_json::<Value>(&vm, "select", &[json!(devices())]).unwrap()["error"],
            error
        );
    }
    let (native, vm, _host) = fixture(true, "own");
    for value in [
        json!({"inputId": "input"}),
        json!({"inputId":"input", "collectionId":"collection", "microphoneId":"microphone", "pluginId":"foreign"}),
        json!({"inputId":"x".repeat(16385), "collectionId":"collection", "microphoneId":"microphone"}),
    ] {
        assert_eq!(
            call_json::<Value>(&vm, "select", &[value]).unwrap()["error"],
            "invalid_device_selection"
        );
    }
    assert!(native.recordings.selected_devices("own").is_none());
}
