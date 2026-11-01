use super::{NativeClientInstallInputs, NativeHostClient, NativeHostClientSlot};
use std::sync::Arc;
use tauri::Manager;

pub(super) async fn retry_inputs(
    inputs: NativeClientInstallInputs,
) -> Result<NativeClientInstallInputs, String> {
    #[cfg(debug_assertions)]
    if matches!(
        inputs.launch_options.instance,
        codeterm_types::server_bootstrap::InstanceSelector::Development { .. }
    ) {
        return tokio::task::spawn_blocking(move || {
            let bytes = std::fs::read(inputs.trusted_resource_root.join("server-component.json"))
                .map_err(|error| error.to_string())?;
            let manifest: codeterm_types::server_bootstrap::ServerComponentManifest =
                serde_json::from_slice(&bytes).map_err(|error| error.to_string())?;
            if !same_client(&inputs.manifest, &manifest) {
                return Err("DEV component changed the running native client identity".into());
            }
            Ok(NativeClientInstallInputs { manifest, ..inputs })
        })
        .await
        .map_err(|error| error.to_string())?;
    }
    Ok(inputs)
}

#[cfg(any(debug_assertions, test))]
fn same_client(
    previous: &codeterm_types::server_bootstrap::ServerComponentManifest,
    current: &codeterm_types::server_bootstrap::ServerComponentManifest,
) -> bool {
    previous.schema == current.schema
        && previous.api_version == current.api_version
        && previous.bootstrap_protocol == current.bootstrap_protocol
        && previous.client_identity == current.client_identity
        && previous.server_executable == current.server_executable
}

pub(super) fn disconnected(slot: &NativeHostClientSlot, client: &Arc<NativeHostClient>) {
    if !slot.clear_if_current(client) {
        return;
    }
    let Some(api) = client
        .app
        .try_state::<super::super::ServerApiAttachmentSlot>()
    else {
        return;
    };
    if api.clear_if_current(&client.ready) {
        crate::startup::frontend_shell::server_disconnected(
            client.app.clone(),
            client.ready.clone(),
            client.server_process.clone(),
        );
    }
}

#[cfg(test)]
mod tests;
