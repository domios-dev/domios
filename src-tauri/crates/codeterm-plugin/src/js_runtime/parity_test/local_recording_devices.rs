use codeterm_local_recording::LocalRecordingTrigger;
use std::sync::Arc;

use serde_json::{json, Value};

use crate::js_runtime::host_compute::tests::AnalyticsHost;
use crate::js_runtime::vm::{call_json, PluginVm};
use crate::{host_traits::PluginHostCtx, local_recording::SelectedDevices};

fn devices() -> SelectedDevices {
    SelectedDevices {
        input_id: "input".into(),
        collection_id: "collection".into(),
        microphone_id: "microphone".into(),
    }
}

fn fixture(granted: bool, _id: &str) -> (Arc<AnalyticsHost>, PluginVm, Arc<dyn PluginHostCtx>) {
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
        granted,
    );
    let host: Arc<dyn PluginHostCtx> = native.clone();
    let vm = PluginVm::from_source(
        "recording-device-probe",
        "module.exports={authority(){return typeof host.localRecording;}}".into(),
    )
    .unwrap();
    (native, vm, host)
}

fn device_call(host: &AnalyticsHost, id: &str, method: &str, selection: Option<Value>) -> Value {
    let reply = match method {
        "list" => host.local_recording_inputs_json(id),
        "saved" => host.local_recording_saved_selection_json(id),
        "select" => {
            let Some(devices) =
                selection.and_then(|value| serde_json::from_value::<SelectedDevices>(value).ok())
            else {
                return json!({"error":"invalid_device_selection"});
            };
            host.local_recording_select_json(id, devices)
        }
        _ => panic!("unknown native device operation"),
    };
    serde_json::from_str(&reply).unwrap()
}

#[test]
fn local_recording_devices_native_js_parity_for_inventory_selection_saved_identity_and_revocation()
{
    let (native, vm, _host) = fixture(true, "own");
    let inputs: Value = serde_json::from_str(&native.local_recording_inputs_json("own")).unwrap();
    assert_eq!(device_call(&native, "own", "list", None), inputs);
    assert_eq!(device_call(&native, "own", "saved", None), Value::Null);
    let selection = devices();
    let generation: Value = device_call(&native, "own", "select", Some(json!(selection)));
    assert_eq!(
        generation,
        json!(native.recordings.status("own").unwrap().generation)
    );
    assert_eq!(device_call(&native, "own", "saved", None), json!(selection));
    assert_eq!(
        device_call(&native, "own", "saved", None),
        serde_json::from_str::<Value>(&native.local_recording_saved_selection_json("own")).unwrap()
    );
    let native_generation: Value =
        serde_json::from_str(&native.local_recording_select_json("own", selection.clone()))
            .unwrap();
    assert!(native_generation.as_u64().unwrap() > generation.as_u64().unwrap());
    assert_eq!(device_call(&native, "own", "saved", None), json!(selection));
    assert_eq!(
        call_json::<Value>(&vm, "authority", &[]).unwrap(),
        json!("undefined")
    );
    native.recordings.revoke("own");
    for method in ["list", "saved"] {
        assert_eq!(
            device_call(&native, "own", method, None)["error"],
            "local_recording_grant_required"
        );
    }
    let refused: Value = device_call(&native, "own", "select", Some(json!(selection)));
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
        let (native, _vm, _host) = fixture(granted, id);
        for method in ["list", "saved"] {
            assert_eq!(device_call(&native, id, method, None)["error"], error);
        }
        assert_eq!(
            device_call(&native, id, "select", Some(json!(devices())))["error"],
            error
        );
    }
    let (native, _vm, _host) = fixture(true, "own");
    for value in [
        json!({"inputId": "input"}),
        json!({"inputId":"input", "collectionId":"collection", "microphoneId":"microphone", "pluginId":"foreign"}),
        json!({"inputId":"x".repeat(16385), "collectionId":"collection", "microphoneId":"microphone"}),
    ] {
        assert_eq!(
            device_call(&native, "own", "select", Some(value.clone()))["error"],
            if value["inputId"].as_str().is_some_and(|id| id.len() > 4096) {
                "devices_unavailable"
            } else {
                "invalid_device_selection"
            }
        );
    }
    assert!(native.recordings.selected_devices("own").is_none());
}
