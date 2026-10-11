mod admission;
pub(crate) mod chat;
mod completion;
pub(crate) mod leases;
pub(crate) mod platform;

pub(crate) use leases::{CaptureLeaseView, RecordingLeases};

use crate::service::ServiceContext;
use codeterm_plugin::local_recording::{RecordingStatus, SelectedDevices};
use tauri::Manager;

impl ServiceContext {
    pub(crate) fn local_recording_subscribe(
        &self,
        plugin_id: &str,
        adapter: &dyn platform::LocalInputAdapter,
    ) -> Result<(), String> {
        if !platform::supported() {
            return Err("local_recording_unsupported".into());
        }
        let declaration = self
            .local_recording_registry
            .declaration(plugin_id)
            .ok_or("trigger_not_declared")?;
        let (devices, generation) = self
            .local_recording_registry
            .selected_devices(plugin_id)
            .ok_or("devices_unavailable")?;
        let service = self
            .app_handle
            .try_state::<crate::service::ServiceState>()
            .ok_or("recording_host_unavailable")?;
        let weak = std::sync::Arc::downgrade(service.inner());
        let id = plugin_id.to_string();
        let callback = std::sync::Arc::new(move |event| {
            let Some(svc) = weak.upgrade() else {
                return;
            };
            let id = id.clone();
            tauri::async_runtime::spawn_blocking(move || {
                if let Err(error) = svc.local_recording_input_event(&id, event) {
                    log::debug!("[local-recording] trigger refused: {error}");
                }
            });
        });
        let subscription = adapter.subscribe(&declaration, &devices, generation, callback)?;
        self.local_recording_registry
            .attach_subscription(plugin_id, generation, subscription)
            .map_err(str::to_string)?;
        if !self
            .local_recording_registry
            .set_available(plugin_id, generation, true)
        {
            return Err("subscription_stale".into());
        }
        Ok(())
    }

    pub(crate) fn local_recording_input_event(
        &self,
        plugin_id: &str,
        event: platform::SelectedInputEvent,
    ) -> Result<(), String> {
        match event {
            platform::SelectedInputEvent::Press { generation } => {
                self.local_recording_signal(plugin_id, generation)
            }
            platform::SelectedInputEvent::Unavailable { generation, reason } => {
                if !self
                    .local_recording_registry
                    .set_available(plugin_id, generation, false)
                {
                    return Err("subscription_stale".into());
                }
                log::debug!("[local-recording] selected input unavailable: {reason:?}");
                let status = self.local_recording_status(plugin_id)?;
                crate::api::events::emit_everywhere(
                    &self.app_handle,
                    "local-recording-status",
                    status,
                );
                Ok(())
            }
        }
    }

    pub(crate) fn local_recording_signal(
        &self,
        plugin_id: &str,
        generation: u64,
    ) -> Result<(), String> {
        let status = self.local_recording_status(plugin_id)?;
        if !status.granted
            || !status.enabled
            || !status.available
            || status.generation != generation
        {
            return Err("subscription_stale".into());
        }
        let event = match self.local_recordings.request_stop(plugin_id) {
            Some(Ok(lease)) => serde_json::json!({ "kind": "stop", "lease": lease }),
            Some(Err(())) => return Ok(()),
            None => {
                let (devices, selected_generation) = self
                    .local_recording_registry
                    .selected_devices(plugin_id)
                    .ok_or("devices_unavailable")?;
                if generation != selected_generation {
                    return Err("subscription_stale".into());
                }
                let lease = self.local_recording_start(plugin_id, &devices, generation)?;
                serde_json::json!({ "kind": "start", "capture": lease })
            }
        };
        crate::api::events::emit_everywhere(&self.app_handle, "local-recording-capture", event);
        Ok(())
    }
    pub(crate) fn local_recording_status(
        &self,
        plugin_id: &str,
    ) -> Result<RecordingStatus, String> {
        self.local_recording_registry
            .status(plugin_id)
            .map_err(str::to_string)
    }

    pub(crate) fn local_recording_set_enabled(
        &self,
        plugin_id: &str,
        enabled: bool,
    ) -> Result<RecordingStatus, String> {
        if enabled && !platform::supported() {
            return Err("local_recording_unsupported".into());
        }
        self.local_recording_registry
            .set_enabled(plugin_id, enabled)
            .map_err(str::to_string)
    }

    pub(crate) fn local_recording_select_devices(
        &self,
        plugin_id: &str,
        devices: SelectedDevices,
    ) -> Result<u64, String> {
        if !platform::supported() {
            return Err("local_recording_unsupported".into());
        }
        self.local_recording_registry
            .select_devices(plugin_id, devices)
            .map_err(str::to_string)
    }

    pub(crate) fn local_recording_start(
        &self,
        plugin_id: &str,
        devices: &SelectedDevices,
        subscription_generation: u64,
    ) -> Result<CaptureLeaseView, String> {
        if !platform::supported() {
            return Err("local_recording_unsupported".into());
        }
        if !self.local_recording_targets.readable() {
            return Err("recording_target_store_unavailable".into());
        }
        let explicit = self.local_recording_targets.get(plugin_id);
        let tab_id = leases::resolve_target(explicit, || {
            self.mux
                .read()
                .resolve_tab_id(super::general_agent::GENERAL_ALIAS)
        })?;
        self.require_managed_send_allowed(&tab_id, None)?;
        let target = self.freeze_recording_target(&tab_id)?;
        self.local_recordings.start(
            plugin_id,
            devices,
            subscription_generation,
            target,
            &self.local_recording_registry,
        )
    }

    pub(crate) fn local_recording_capture_stopped(&self, lease: &str) -> Result<(), String> {
        self.local_recordings.stopped(lease)
    }

    pub(crate) fn local_recording_cancel(&self, lease: &str) {
        self.local_recordings.cancel(lease);
    }
}

pub(crate) fn host_json<T: serde::Serialize>(result: Result<T, String>) -> String {
    match result {
        Ok(value) => serde_json::to_string(&value).unwrap_or_else(|_| "null".into()),
        Err(error) => serde_json::json!({ "error": error }).to_string(),
    }
}
