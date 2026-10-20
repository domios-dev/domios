mod admission;
pub(crate) mod chat;
mod clip;
mod completion;
mod inventory;
pub(crate) mod leases;
pub(crate) mod platform;
mod receiver_status;
mod selections;

pub use admission::LocalRecordingCommitOutcome;
pub use clip::CaptureClipDescriptor;
pub(crate) use leases::CaptureLeaseView;
pub use leases::RecordingLeases;
pub(crate) use selections::RecordingSelectionStore;

use crate::service::ServiceContext;
use codeterm_plugin::local_recording::{
    LocalInputSetup, LocalRecordingInvalidationReason, LocalRecordingRegistryHandle,
    RecordingStatus, SelectedDevices,
};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::Duration;

const SETUP_DEADLINE: Duration = Duration::from_secs(10);
const CLEANUP_DEADLINE: Duration = Duration::from_secs(5);

impl ServiceContext {
    pub(crate) fn install_local_recording_invalidation_observer(&self) -> Result<(), String> {
        let weak = self.weak_self();
        self.ext::<codeterm_plugin::local_recording::LocalRecordingRegistryHandle>()
            .set_invalidation_observer(Arc::new(move |invalidation| {
                let Some(service) = weak.upgrade() else {
                    return;
                };
                service
                    .local_recordings
                    .invalidate_generation(&invalidation.plugin_id, invalidation.generation);
                service.ui.emit_native(
                    "local-recording-capture-invalidated",
                    serde_json::to_value(&invalidation).unwrap_or_default(),
                );
            }));
        Ok(())
    }

    pub(crate) async fn local_recording_subscribe(
        &self,
        plugin_id: &str,
        adapter: Arc<dyn platform::LocalInputAdapter>,
    ) -> Result<(), String> {
        self.install_local_recording_invalidation_observer()?;
        if !platform::supported() {
            return Err("local_recording_unsupported".into());
        }
        let weak = self.weak_self();
        let id = plugin_id.to_string();
        let callback = std::sync::Arc::new(move |event| {
            let Some(svc) = weak.upgrade() else {
                return;
            };
            let id = id.clone();
            crate::async_runtime::spawn_blocking(move || {
                if let Err(error) = svc.local_recording_input_event(&id, event) {
                    log::debug!("[local-recording] trigger refused: {error}");
                }
            });
        });
        subscribe_local_input(
            self.ext::<codeterm_plugin::local_recording::LocalRecordingRegistryHandle>()
                .clone(),
            plugin_id.to_string(),
            adapter,
            callback,
        )
        .await
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
                let reason = match reason {
                    platform::InputUnavailable::Removed => {
                        LocalRecordingInvalidationReason::DeviceRemoved
                    }
                    platform::InputUnavailable::AccessLost => {
                        LocalRecordingInvalidationReason::AccessLost
                    }
                    platform::InputUnavailable::Error => {
                        LocalRecordingInvalidationReason::DeviceUnavailable
                    }
                };
                if !self
                    .ext::<codeterm_plugin::local_recording::LocalRecordingRegistryHandle>()
                    .mark_unavailable_with_reason(plugin_id, generation, reason)
                {
                    return Err("subscription_stale".into());
                }
                log::debug!("[local-recording] selected input unavailable: {reason:?}");
                let status = self.local_recording_status(plugin_id)?;
                self.ui.emit(
                    "local-recording-status",
                    serde_json::to_value(status).unwrap_or_default(),
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
                    .ext::<codeterm_plugin::local_recording::LocalRecordingRegistryHandle>()
                    .selected_devices(plugin_id)
                    .ok_or("devices_unavailable")?;
                if generation != selected_generation {
                    return Err("subscription_stale".into());
                }
                let lease = self.local_recording_start(plugin_id, &devices, generation)?;
                serde_json::json!({ "kind": "start", "capture": lease })
            }
        };
        self.ui.emit("local-recording-capture", event);
        Ok(())
    }
    pub fn local_recording_status(&self, plugin_id: &str) -> Result<RecordingStatus, String> {
        self.ext::<codeterm_plugin::local_recording::LocalRecordingRegistryHandle>()
            .status(plugin_id)
            .map_err(str::to_string)
    }

    pub fn local_recording_set_enabled(
        &self,
        plugin_id: &str,
        enabled: bool,
    ) -> Result<RecordingStatus, String> {
        self.local_recording_set_enabled_with_adapter(
            plugin_id,
            enabled,
            platform::production_adapter(),
        )
        .map(|(status, _)| status)
    }

    fn local_recording_set_enabled_with_adapter(
        &self,
        plugin_id: &str,
        enabled: bool,
        adapter: Arc<dyn platform::LocalInputAdapter>,
    ) -> Result<(RecordingStatus, Option<Receiver<()>>), String> {
        if enabled && !platform::supported() {
            return Err("local_recording_unsupported".into());
        }
        let status = self
            .ext::<codeterm_plugin::local_recording::LocalRecordingRegistryHandle>()
            .set_enabled(plugin_id, enabled)
            .map_err(str::to_string)?;
        self.ui.emit(
            "local-recording-status",
            serde_json::to_value(&status).unwrap_or_default(),
        );
        let completion =
            (enabled && status.granted && status.devices_selected && !status.available)
                .then(|| self.schedule_local_recording_subscription(plugin_id.into(), adapter));
        Ok((status, completion))
    }

    pub fn local_recording_select_devices(
        &self,
        plugin_id: &str,
        devices: SelectedDevices,
    ) -> Result<u64, String> {
        if !platform::supported() {
            return Err("local_recording_unsupported".into());
        }
        if !self.local_recording_status(plugin_id)?.granted {
            return Err("local_recording_grant_required".into());
        }
        if !devices.valid() {
            return Err("invalid_device_selection".into());
        }
        let selection = devices.clone();
        let generation = self
            .ext::<codeterm_plugin::local_recording::LocalRecordingRegistryHandle>()
            .select_devices_persisted(plugin_id, devices, || {
                self.ext::<Arc<crate::service::local_recording::RecordingSelectionStore>>()
                    .set(plugin_id, &selection)
            })?;
        let status = self.local_recording_status(plugin_id)?;
        if status.enabled && status.granted && !status.available {
            self.schedule_local_recording_subscription(
                plugin_id.into(),
                platform::production_adapter(),
            );
        }
        Ok(generation)
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
        if !self
            .ext::<crate::service::receiver_target::LocalRecordingTargets>()
            .0
            .readable()
        {
            return Err("recording_target_store_unavailable".into());
        }
        let explicit = self
            .ext::<crate::service::receiver_target::LocalRecordingTargets>()
            .0
            .get(plugin_id);
        let tab_id = leases::resolve_target(explicit, || {
            self.mux
                .read()
                .resolve_tab_id(super::general_agent::GENERAL_ALIAS)
        })?;
        self.require_managed_send_allowed(&tab_id, None)?;
        let target = self.freeze_recording_target(&tab_id)?;
        self.install_local_recording_invalidation_observer()?;
        self.local_recordings.start(
            plugin_id,
            devices,
            subscription_generation,
            target,
            self.ext::<codeterm_plugin::local_recording::LocalRecordingRegistryHandle>(),
        )
    }

    pub fn local_recording_capture_stopped(&self, lease: &str) -> Result<(), String> {
        self.local_recordings.stopped(lease)
    }

    pub fn local_recording_cancel(&self, lease: &str) {
        self.local_recordings.cancel(lease);
    }

    fn schedule_local_recording_subscription(
        &self,
        plugin_id: String,
        adapter: Arc<dyn platform::LocalInputAdapter>,
    ) -> Receiver<()> {
        let weak = self.weak_self();
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        crate::async_runtime::spawn(async move {
            let service = weak.upgrade();
            let result = match service {
                Some(service) => {
                    let result = service.local_recording_subscribe(&plugin_id, adapter).await;
                    if let Ok(status) = service.local_recording_status(&plugin_id) {
                        service.ui.emit(
                            "local-recording-status",
                            serde_json::to_value(status).unwrap_or_default(),
                        );
                    }
                    result
                }
                None => Err("recording_host_unavailable".into()),
            };
            if let Err(error) = result {
                log::debug!("[local-recording] subscription setup refused: {error}");
            }
            let _ = finished_tx.send(());
        });
        finished_rx
    }
}

struct SetupCancellation {
    registry: LocalRecordingRegistryHandle,
    setup: LocalInputSetup,
    armed: bool,
}

impl Drop for SetupCancellation {
    fn drop(&mut self) {
        if self.armed {
            self.registry.cancel_subscription_setup(&self.setup);
        }
    }
}

async fn subscribe_local_input(
    registry: LocalRecordingRegistryHandle,
    plugin_id: String,
    adapter: Arc<dyn platform::LocalInputAdapter>,
    callback: Arc<dyn Fn(platform::SelectedInputEvent) + Send + Sync>,
) -> Result<(), String> {
    let cleanup_registry = registry.clone();
    let cleanup_id = plugin_id.clone();
    crate::async_runtime::spawn_blocking(move || {
        cleanup_registry.wait_for_subscription_slot(&cleanup_id, CLEANUP_DEADLINE)
    })
    .await
    .map_err(|_| "recording_cleanup_worker_failed".to_string())?
    .map_err(str::to_string)?;

    let setup = registry
        .begin_subscription_setup(&plugin_id)
        .map_err(str::to_string)?;
    let mut cancellation = SetupCancellation {
        registry: registry.clone(),
        setup: setup.clone(),
        armed: true,
    };
    let setup_registry = registry.clone();
    let setup_callback = callback.clone();
    let worker = crate::async_runtime::spawn_blocking(move || {
        match adapter.subscribe(
            &setup.declaration,
            &setup.devices,
            setup.generation,
            setup_callback,
        ) {
            Ok(subscription) => setup_registry
                .complete_subscription_setup(&setup, subscription)
                .map_err(str::to_string),
            Err(error) => Err(setup_registry.fail_subscription_setup(&setup, error)),
        }
    });
    match tokio::time::timeout(SETUP_DEADLINE, worker).await {
        Ok(Ok(Ok(()))) => {
            cancellation.armed = false;
            Ok(())
        }
        Ok(Ok(Err(error))) => Err(error),
        Ok(Err(_)) => Err("recording_setup_worker_failed".into()),
        Err(_) => Err("local_recording_setup_timeout".into()),
    }
}

pub(crate) fn host_json<T: serde::Serialize>(result: Result<T, String>) -> String {
    match result {
        Ok(value) => serde_json::to_string(&value).unwrap_or_else(|_| "null".into()),
        Err(error) => serde_json::json!({ "error": error }).to_string(),
    }
}

#[cfg(test)]
pub(crate) fn arm_registry_fixture(
    registry: &codeterm_plugin::local_recording::LocalRecordingRegistry,
    plugin_id: &str,
) -> u64 {
    struct ReadySubscription;

    impl platform::LocalInputSubscription for ReadySubscription {
        fn request_stop(&mut self) -> platform::SubscriptionTeardown {
            let (sender, receiver) = std::sync::mpsc::channel();
            sender.send(Ok(())).unwrap();
            platform::SubscriptionTeardown::new(receiver)
        }
    }

    let setup = registry.begin_subscription_setup(plugin_id).unwrap();
    let generation = setup.generation;
    registry
        .complete_subscription_setup(&setup, Box::new(ReadySubscription))
        .unwrap();
    generation
}

#[cfg(test)]
mod setup_tests;
