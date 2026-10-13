use std::sync::Arc;

use rquickjs::{function::Func, Object};

use super::{host_api::PluginSandbox, vm::VmError};
use crate::local_recording::SelectedDevices;

fn error(message: &str) -> String {
    serde_json::json!({ "error": message }).to_string()
}

pub(super) fn install(host: &Object<'_>, sandbox: &Arc<PluginSandbox>) -> Result<(), VmError> {
    let sb = sandbox.clone();
    host.set(
        "localRecordingInputs",
        Func::new(move || -> String {
            if !sb.local_recording_allowed() {
                return error("local_recording_grant_required");
            }
            match sb.service() {
                Some(service) => service.local_recording_inputs_json(sb.plugin_id()),
                None => error("host service unavailable"),
            }
        }),
    )
    .map_err(|error| VmError::Script(error.to_string()))?;
    let sb = sandbox.clone();
    host.set(
        "localRecordingSelect",
        Func::new(move |json: String| -> String {
            if !sb.local_recording_allowed() {
                return error("local_recording_grant_required");
            }
            if json.len() > 16384 {
                return error("invalid_device_selection");
            }
            let Ok(devices) = serde_json::from_str::<SelectedDevices>(&json) else {
                return error("invalid_device_selection");
            };
            if !devices.valid() {
                return error("invalid_device_selection");
            }
            match sb.service() {
                Some(service) => service.local_recording_select_json(sb.plugin_id(), devices),
                None => error("host service unavailable"),
            }
        }),
    )
    .map_err(|error| VmError::Script(error.to_string()))?;
    let sb = sandbox.clone();
    host.set(
        "localRecordingSavedSelection",
        Func::new(move || -> String {
            if !sb.local_recording_allowed() {
                return error("local_recording_grant_required");
            }
            match sb.service() {
                Some(service) => service.local_recording_saved_selection_json(sb.plugin_id()),
                None => error("host service unavailable"),
            }
        }),
    )
    .map_err(|error| VmError::Script(error.to_string()))
}
