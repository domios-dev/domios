use super::platform::{self, DeviceArrivalEvent, LocalInputAdapter, LocalInputArrivalWatch};
use super::{RecordingSelectionStore, CLEANUP_DEADLINE};
use crate::service::ServiceContext;
use codeterm_local_recording::LocalRecordingTrigger;
use codeterm_plugin::local_recording::{
    LocalRecordingRegistryHandle, RecordingInputDevice, RecordingInputKind, SelectedDevices,
    SubscriptionTeardown, SubscriptionTeardownWait,
};
use parking_lot::Mutex;
use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReconcileMode {
    AttachOrWatch,
    WatchOnly,
}

impl ReconcileMode {
    fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::WatchOnly, Self::WatchOnly) => Self::WatchOnly,
            _ => Self::AttachOrWatch,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CandidateResolution {
    Exact(SelectedDevices),
    Rebound(SelectedDevices),
    Absent,
    Ambiguous,
}

pub(crate) fn windows_hid_hardware_class(input_id: &str) -> Option<String> {
    let upper = input_id.to_ascii_uppercase();
    if !upper.starts_with(r"\\?\HID#") {
        return None;
    }
    let hardware = upper.split('#').nth(1)?;
    (hardware.contains("VID_") && hardware.contains("PID_")).then(|| hardware.to_string())
}

fn trigger_windows_hardware_class(trigger: &LocalRecordingTrigger) -> Option<String> {
    let button = trigger.button.as_ref()?;
    Some(format!(
        "VID_{:04X}&PID_{:04X}&MI_{:02X}&{}",
        trigger.vendor_id,
        trigger.product_id,
        button.interface_number,
        button.collection_id.to_ascii_uppercase()
    ))
}

pub(crate) fn resolve_candidate(
    trigger: &LocalRecordingTrigger,
    persisted: &SelectedDevices,
    candidates: &[RecordingInputDevice],
) -> CandidateResolution {
    let connected: Vec<&RecordingInputDevice> = candidates
        .iter()
        .filter(|candidate| candidate.connected && candidate.kind == RecordingInputKind::Hid)
        .collect();
    let exact: Vec<&RecordingInputDevice> = connected
        .iter()
        .copied()
        .filter(|candidate| {
            candidate.input_id.eq_ignore_ascii_case(&persisted.input_id)
                && candidate
                    .collection_id
                    .eq_ignore_ascii_case(&persisted.collection_id)
        })
        .collect();
    match exact.as_slice() {
        [_] => return CandidateResolution::Exact(persisted.clone()),
        [_, ..] => return CandidateResolution::Ambiguous,
        [] => {}
    }

    let persisted_hw = windows_hid_hardware_class(&persisted.input_id);
    if let (Some(hw), Some(expected)) = (
        persisted_hw.as_deref(),
        trigger_windows_hardware_class(trigger),
    ) {
        if hw != expected {
            return CandidateResolution::Absent;
        }
    }
    let persisted_is_macos = persisted.input_id.starts_with("macos-hid-v1:");
    let fallback: Vec<&RecordingInputDevice> = connected
        .into_iter()
        .filter(|candidate| {
            if !candidate
                .collection_id
                .eq_ignore_ascii_case(&persisted.collection_id)
            {
                return false;
            }
            if let Some(button) = trigger.button.as_ref() {
                if !candidate
                    .collection_id
                    .eq_ignore_ascii_case(&button.collection_id)
                {
                    return false;
                }
            }
            match persisted_hw.as_deref() {
                Some(hw) => windows_hid_hardware_class(&candidate.input_id).as_deref() == Some(hw),
                None if persisted_is_macos => candidate.input_id.starts_with("macos-hid-v1:"),
                None => windows_hid_hardware_class(&candidate.input_id).is_none(),
            }
        })
        .collect();
    match fallback.as_slice() {
        [] => CandidateResolution::Absent,
        [only] => {
            let rebound = SelectedDevices {
                input_id: only.input_id.clone(),
                collection_id: only.collection_id.clone(),
                microphone_id: persisted.microphone_id.clone(),
            };
            if rebound.valid() {
                CandidateResolution::Rebound(rebound)
            } else {
                CandidateResolution::Absent
            }
        }
        [_, ..] => CandidateResolution::Ambiguous,
    }
}

#[derive(Default)]
pub(crate) struct LocalRecordingHotplug {
    state: Mutex<HotplugState>,
}

#[derive(Default)]
struct HotplugState {
    watchers: BTreeMap<String, WatcherEntry>,
    in_flight: BTreeMap<String, InFlightWork>,
}

struct WatcherEntry {
    generation: u64,
    phase: WatcherPhase,
}

enum WatcherPhase {
    Starting {
        cancelled: bool,
        done_waiters: Vec<Sender<()>>,
    },
    Active(Box<dyn LocalInputArrivalWatch>),
    Stopping(SubscriptionTeardown),
}

struct InFlightWork {
    next: Option<(ReconcileMode, Arc<dyn LocalInputAdapter>)>,
    waiters: Vec<Sender<()>>,
}

struct InFlightGuard {
    hotplug: Arc<LocalRecordingHotplug>,
    plugin_id: String,
    armed: bool,
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let waiters = self
            .hotplug
            .state
            .lock()
            .in_flight
            .remove(&self.plugin_id)
            .map(|work| work.waiters)
            .unwrap_or_default();
        for waiter in waiters {
            let _ = waiter.send(());
        }
    }
}

impl LocalRecordingHotplug {
    fn cancel_watcher_now(self: &Arc<Self>, plugin_id: &str) {
        let extracted = {
            let mut state = self.state.lock();
            match state.watchers.get_mut(plugin_id) {
                Some(entry) => match &mut entry.phase {
                    WatcherPhase::Starting { cancelled, .. } => {
                        *cancelled = true;
                        None
                    }
                    WatcherPhase::Active(_) => {
                        let generation = entry.generation;
                        let previous = std::mem::replace(
                            &mut entry.phase,
                            WatcherPhase::Starting {
                                cancelled: true,
                                done_waiters: Vec::new(),
                            },
                        );
                        match previous {
                            WatcherPhase::Active(watch) => Some((generation, watch)),
                            _ => None,
                        }
                    }
                    WatcherPhase::Stopping(_) => None,
                },
                None => None,
            }
        };
        let Some((generation, mut watch)) = extracted else {
            return;
        };
        let teardown = watch.request_stop();
        let waiters = {
            let mut state = self.state.lock();
            let waiters = match state.watchers.get_mut(plugin_id) {
                Some(entry) if entry.generation == generation => {
                    let prev = std::mem::replace(
                        &mut entry.phase,
                        WatcherPhase::Stopping(teardown.clone()),
                    );
                    match prev {
                        WatcherPhase::Starting { done_waiters, .. } => done_waiters,
                        _ => Vec::new(),
                    }
                }
                _ => Vec::new(),
            };
            waiters
        };
        for waiter in waiters {
            let _ = waiter.send(());
        }
        spawn_watcher_reaper(self.clone(), plugin_id.to_string(), generation, teardown);
    }

    fn stop_watcher_blocking(
        self: &Arc<Self>,
        plugin_id: &str,
        timeout: Duration,
    ) -> Result<(), &'static str> {
        let deadline = Instant::now() + timeout;
        loop {
            let action = {
                let mut state = self.state.lock();
                match state.watchers.get_mut(plugin_id) {
                    None => return Ok(()),
                    Some(entry) => match &mut entry.phase {
                        WatcherPhase::Starting {
                            cancelled,
                            done_waiters,
                        } => {
                            *cancelled = true;
                            let (tx, rx) = mpsc::channel();
                            done_waiters.push(tx);
                            StopAction::WaitStart(rx)
                        }
                        WatcherPhase::Active(_) => {
                            let generation = entry.generation;
                            let prev = std::mem::replace(
                                &mut entry.phase,
                                WatcherPhase::Starting {
                                    cancelled: true,
                                    done_waiters: Vec::new(),
                                },
                            );
                            match prev {
                                WatcherPhase::Active(watch) => {
                                    StopAction::RequestStop(generation, watch)
                                }
                                _ => unreachable!(),
                            }
                        }
                        WatcherPhase::Stopping(teardown) => {
                            StopAction::WaitTeardown(entry.generation, teardown.clone())
                        }
                    },
                }
            };
            match action {
                StopAction::WaitStart(rx) => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if rx.recv_timeout(remaining).is_err() {
                        return Err("arrival_watch_cleanup_pending");
                    }
                }
                StopAction::RequestStop(generation, mut watch) => {
                    let teardown = watch.request_stop();
                    let waiters = {
                        let mut state = self.state.lock();
                        match state.watchers.get_mut(plugin_id) {
                            Some(entry) if entry.generation == generation => {
                                let prev = std::mem::replace(
                                    &mut entry.phase,
                                    WatcherPhase::Stopping(teardown.clone()),
                                );
                                match prev {
                                    WatcherPhase::Starting { done_waiters, .. } => done_waiters,
                                    _ => Vec::new(),
                                }
                            }
                            _ => Vec::new(),
                        }
                    };
                    for waiter in waiters {
                        let _ = waiter.send(());
                    }
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    match teardown.wait_timeout(remaining) {
                        SubscriptionTeardownWait::Complete => {
                            self.remove_stopped_watcher(plugin_id, generation);
                            return Ok(());
                        }
                        SubscriptionTeardownWait::Pending | SubscriptionTeardownWait::Failed(_) => {
                            return Err("arrival_watch_cleanup_pending");
                        }
                    }
                }
                StopAction::WaitTeardown(generation, teardown) => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    match teardown.wait_timeout(remaining) {
                        SubscriptionTeardownWait::Complete => {
                            self.remove_stopped_watcher(plugin_id, generation);
                            return Ok(());
                        }
                        SubscriptionTeardownWait::Pending | SubscriptionTeardownWait::Failed(_) => {
                            return Err("arrival_watch_cleanup_pending");
                        }
                    }
                }
            }
        }
    }

    fn remove_stopped_watcher(&self, plugin_id: &str, generation: u64) {
        let mut state = self.state.lock();
        let should_remove = state.watchers.get(plugin_id).is_some_and(|entry| {
            entry.generation == generation && matches!(entry.phase, WatcherPhase::Stopping(_))
        });
        if should_remove {
            state.watchers.remove(plugin_id);
        }
    }

    #[cfg(all(test, any(windows, target_os = "macos")))]
    pub(crate) fn has_active_watcher(&self, plugin_id: &str) -> bool {
        self.state
            .lock()
            .watchers
            .get(plugin_id)
            .is_some_and(|entry| matches!(entry.phase, WatcherPhase::Active(_)))
    }

    #[cfg(all(test, any(windows, target_os = "macos")))]
    pub(crate) fn wait_in_flight(&self, plugin_id: &str) -> Option<Receiver<()>> {
        let mut state = self.state.lock();
        let work = state.in_flight.get_mut(plugin_id)?;
        let (tx, rx) = mpsc::channel();
        work.waiters.push(tx);
        Some(rx)
    }
}

enum StopAction {
    WaitStart(Receiver<()>),
    RequestStop(u64, Box<dyn LocalInputArrivalWatch>),
    WaitTeardown(u64, SubscriptionTeardown),
}

fn spawn_watcher_reaper(
    hotplug: Arc<LocalRecordingHotplug>,
    plugin_id: String,
    generation: u64,
    teardown: SubscriptionTeardown,
) {
    crate::async_runtime::spawn_blocking(move || {
        if matches!(
            teardown.wait_timeout(CLEANUP_DEADLINE),
            SubscriptionTeardownWait::Complete
        ) {
            hotplug.remove_stopped_watcher(&plugin_id, generation);
        }
    });
}

fn should_watch_after_subscribe_error(error: &str) -> bool {
    !matches!(
        error,
        "local_recording_unsupported"
            | "local_input_descriptor_unsupported"
            | "local_recording_grant_required"
            | "trigger_disabled"
            | "trigger_not_declared"
            | "windows_input_access_lost"
            | "macos_input_access_denied"
            | "macos_input_access_not_granted"
            | "macos_hid_element_mapping_unavailable"
    )
}

impl ServiceContext {
    pub fn restore_local_recording_plugins(&self) {
        let _ = self.install_local_recording_invalidation_observer();
        let ids = self.ext::<LocalRecordingRegistryHandle>().plugin_ids();
        for id in ids {
            self.restore_local_recording_plugin(&id);
        }
    }

    pub(crate) fn restore_local_recording_plugin(&self, plugin_id: &str) {
        let _ = self
            .restore_local_recording_plugin_with_adapter(plugin_id, platform::production_adapter());
    }

    pub(crate) fn restore_local_recording_plugin_with_adapter(
        &self,
        plugin_id: &str,
        adapter: Arc<dyn LocalInputAdapter>,
    ) -> Option<Receiver<()>> {
        let _ = self.install_local_recording_invalidation_observer();
        if !platform::supported() {
            return None;
        }
        let Ok(status) = self.local_recording_status(plugin_id) else {
            self.cancel_local_recording_arrival_watch(plugin_id);
            return None;
        };
        if !status.granted {
            self.cancel_local_recording_arrival_watch(plugin_id);
            self.ui.emit(
                "local-recording-status",
                serde_json::to_value(status).unwrap_or_default(),
            );
            return None;
        }
        let store = self.ext::<Arc<RecordingSelectionStore>>();
        let Ok(Some(devices)) = store.get(plugin_id) else {
            return None;
        };
        let Ok(enabled) = store.enabled(plugin_id) else {
            return None;
        };
        let Ok(restored) = self
            .ext::<LocalRecordingRegistryHandle>()
            .restore_persisted(plugin_id, devices, enabled)
        else {
            return None;
        };
        self.ui.emit(
            "local-recording-status",
            serde_json::to_value(&restored).unwrap_or_default(),
        );
        (restored.granted && restored.enabled && restored.devices_selected && !restored.available)
            .then(|| {
                self.schedule_local_recording_reconcile(
                    plugin_id.into(),
                    adapter,
                    ReconcileMode::AttachOrWatch,
                )
            })
    }

    pub(crate) fn cancel_local_recording_arrival_watch(&self, plugin_id: &str) {
        self.ext::<Arc<LocalRecordingHotplug>>()
            .cancel_watcher_now(plugin_id);
    }

    pub(crate) fn schedule_local_recording_reconcile(
        &self,
        plugin_id: String,
        adapter: Arc<dyn LocalInputAdapter>,
        mode: ReconcileMode,
    ) -> Receiver<()> {
        let hotplug = self.ext::<Arc<LocalRecordingHotplug>>().clone();
        let (finished_tx, finished_rx) = mpsc::channel();
        {
            let mut state = hotplug.state.lock();
            if let Some(existing) = state.in_flight.get_mut(&plugin_id) {
                existing.next = Some(match existing.next.take() {
                    Some((prev, _)) => (prev.merge(mode), adapter),
                    None => (mode, adapter),
                });
                existing.waiters.push(finished_tx);
                return finished_rx;
            }
            state.in_flight.insert(
                plugin_id.clone(),
                InFlightWork {
                    next: None,
                    waiters: vec![finished_tx],
                },
            );
        }
        let weak = self.weak_self();
        crate::async_runtime::spawn(async move {
            let mut guard = InFlightGuard {
                hotplug: hotplug.clone(),
                plugin_id: plugin_id.clone(),
                armed: true,
            };
            let mut current_mode = mode;
            let mut current_adapter = adapter;
            loop {
                let Some(service) = weak.upgrade() else {
                    break;
                };
                service
                    .reconcile_local_recording_once(
                        &plugin_id,
                        current_adapter.clone(),
                        current_mode,
                    )
                    .await;
                let next = {
                    let mut state = hotplug.state.lock();
                    match state.in_flight.get_mut(&plugin_id) {
                        Some(work) => work.next.take(),
                        None => None,
                    }
                };
                match next {
                    Some((next_mode, next_adapter)) => {
                        current_mode = next_mode;
                        current_adapter = next_adapter;
                    }
                    None => {
                        let waiters = hotplug
                            .state
                            .lock()
                            .in_flight
                            .remove(&plugin_id)
                            .map(|work| work.waiters)
                            .unwrap_or_default();
                        guard.armed = false;
                        for waiter in waiters {
                            let _ = waiter.send(());
                        }
                        break;
                    }
                }
            }
        });
        finished_rx
    }

    async fn reconcile_local_recording_once(
        &self,
        plugin_id: &str,
        adapter: Arc<dyn LocalInputAdapter>,
        mode: ReconcileMode,
    ) {
        let hotplug = self.ext::<Arc<LocalRecordingHotplug>>().clone();
        if !platform::supported() {
            let _ = stop_watcher_async(hotplug, plugin_id).await;
            return;
        }
        if !self.is_eligible_for_auto_attach(plugin_id) {
            let _ = stop_watcher_async(hotplug, plugin_id).await;
            return;
        }
        let registry = self.ext::<LocalRecordingRegistryHandle>().clone();
        let wait_id = plugin_id.to_string();
        let slot_ready = crate::async_runtime::spawn_blocking(move || {
            registry.wait_for_subscription_slot(&wait_id, CLEANUP_DEADLINE)
        })
        .await;
        if !matches!(slot_ready, Ok(Ok(()))) {
            return;
        }
        if !self.is_eligible_for_auto_attach(plugin_id) {
            let _ = stop_watcher_async(hotplug, plugin_id).await;
            return;
        }
        if mode == ReconcileMode::WatchOnly {
            self.start_arrival_watch(plugin_id, adapter, true).await;
            return;
        }
        if stop_watcher_async(hotplug.clone(), plugin_id)
            .await
            .is_err()
        {
            return;
        }
        let Some((declaration, persisted, expected_generation)) =
            self.eligible_attach_inputs(plugin_id)
        else {
            return;
        };
        let enum_adapter = adapter.clone();
        let enum_decl = declaration.clone();
        let enumerated =
            crate::async_runtime::spawn_blocking(move || enum_adapter.enumerate_inputs(&enum_decl))
                .await;
        match enumerated {
            Ok(Ok(Some(candidates))) => {
                match resolve_candidate(&declaration, &persisted, &candidates) {
                    CandidateResolution::Exact(_) => {}
                    CandidateResolution::Rebound(new_devices) => {
                        let store = self.ext::<Arc<RecordingSelectionStore>>().clone();
                        let registry = self.ext::<LocalRecordingRegistryHandle>().clone();
                        let rebind_id = plugin_id.to_string();
                        let rebind = crate::async_runtime::spawn_blocking(move || {
                            registry.rebind_selected_devices_if_eligible(
                                &rebind_id,
                                expected_generation,
                                new_devices.clone(),
                                || store.set(&rebind_id, &new_devices),
                            )
                        })
                        .await
                        .map_err(|_| "recording_rebind_worker_failed".to_string())
                        .and_then(|result| result);
                        if let Err(error) = rebind {
                            log::debug!("[local-recording] receiver rebind refused: {error}");
                            self.emit_local_recording_status(plugin_id);
                            if self.is_eligible_for_auto_attach(plugin_id) {
                                self.start_arrival_watch(plugin_id, adapter, false).await;
                            }
                            return;
                        }
                    }
                    CandidateResolution::Ambiguous => {
                        log::debug!(
                            "[local-recording] multiple receiver candidates matched {plugin_id}; refusing auto-attach"
                        );
                        self.emit_local_recording_status(plugin_id);
                        self.start_arrival_watch(plugin_id, adapter, false).await;
                        return;
                    }
                    CandidateResolution::Absent => {
                        self.emit_local_recording_status(plugin_id);
                        self.start_arrival_watch(plugin_id, adapter, true).await;
                        return;
                    }
                }
            }
            Ok(Ok(None)) => {}
            Ok(Err(error)) => {
                log::debug!("[local-recording] input inventory failed: {error}");
                self.emit_local_recording_status(plugin_id);
                self.start_arrival_watch(plugin_id, adapter, false).await;
                return;
            }
            Err(_) => {
                self.emit_local_recording_status(plugin_id);
                return;
            }
        }

        let result = self
            .local_recording_subscribe(plugin_id, adapter.clone())
            .await;
        self.emit_local_recording_status(plugin_id);
        if let Err(error) = result {
            log::debug!("[local-recording] subscription setup refused: {error}");
            let registry = self.ext::<LocalRecordingRegistryHandle>().clone();
            let wait_id = plugin_id.to_string();
            let _ = crate::async_runtime::spawn_blocking(move || {
                registry.wait_for_subscription_slot(&wait_id, CLEANUP_DEADLINE)
            })
            .await;
            if should_watch_after_subscribe_error(&error)
                && self.is_eligible_for_auto_attach(plugin_id)
            {
                self.start_arrival_watch(plugin_id, adapter, false).await;
            }
        }
    }

    async fn start_arrival_watch(
        &self,
        plugin_id: &str,
        adapter: Arc<dyn LocalInputAdapter>,
        recheck_gap: bool,
    ) {
        let Some((_, _, generation)) = self.eligible_attach_inputs(plugin_id) else {
            return;
        };
        let hotplug = self.ext::<Arc<LocalRecordingHotplug>>().clone();
        let already_active = hotplug
            .state
            .lock()
            .watchers
            .get(plugin_id)
            .is_some_and(|existing| {
                existing.generation == generation
                    && matches!(existing.phase, WatcherPhase::Active(_))
            });
        if already_active {
            return;
        }
        if stop_watcher_async(hotplug.clone(), plugin_id)
            .await
            .is_err()
        {
            return;
        }
        let Some((declaration, persisted, generation)) = self.eligible_attach_inputs(plugin_id)
        else {
            return;
        };
        {
            let mut state = hotplug.state.lock();
            if state.watchers.contains_key(plugin_id) {
                return;
            }
            state.watchers.insert(
                plugin_id.into(),
                WatcherEntry {
                    generation,
                    phase: WatcherPhase::Starting {
                        cancelled: false,
                        done_waiters: Vec::new(),
                    },
                },
            );
        }
        let weak = self.weak_self();
        let callback_id = plugin_id.to_string();
        let callback_adapter = adapter.clone();
        let callback = Arc::new(move |_event: DeviceArrivalEvent| {
            let Some(service) = weak.upgrade() else {
                return;
            };
            let _ = service.schedule_local_recording_reconcile(
                callback_id.clone(),
                callback_adapter.clone(),
                ReconcileMode::AttachOrWatch,
            );
        });
        let watch_decl = declaration.clone();
        let watch_adapter = adapter.clone();
        let started = crate::async_runtime::spawn_blocking(move || {
            watch_adapter.watch_arrivals(&watch_decl, callback)
        })
        .await;
        let result = match started {
            Ok(result) => result,
            Err(_) => {
                finish_failed_start(&hotplug, plugin_id, generation, None);
                return;
            }
        };
        match result {
            Ok(watch) => {
                let eligible_now = self.local_recording_status(plugin_id).is_ok_and(|status| {
                    status.granted
                        && status.enabled
                        && status.devices_selected
                        && !status.available
                        && !status.capture_active
                        && status.generation == generation
                });
                let (accepted, waiters) = {
                    let mut state = hotplug.state.lock();
                    match state.watchers.get_mut(plugin_id) {
                        Some(entry)
                            if entry.generation == generation
                                && matches!(
                                    entry.phase,
                                    WatcherPhase::Starting {
                                        cancelled: false,
                                        ..
                                    }
                                )
                                && eligible_now =>
                        {
                            let prev =
                                std::mem::replace(&mut entry.phase, WatcherPhase::Active(watch));
                            let waiters = match prev {
                                WatcherPhase::Starting { done_waiters, .. } => done_waiters,
                                _ => Vec::new(),
                            };
                            (None, waiters)
                        }
                        _ => (Some(watch), Vec::new()),
                    }
                };
                if let Some(mut rejected) = accepted {
                    let teardown = rejected.request_stop();
                    let waiters = finish_failed_start(
                        &hotplug,
                        plugin_id,
                        generation,
                        Some(teardown.clone()),
                    );
                    for waiter in waiters {
                        let _ = waiter.send(());
                    }
                    spawn_watcher_reaper(hotplug, plugin_id.to_string(), generation, teardown);
                    return;
                }
                for waiter in waiters {
                    let _ = waiter.send(());
                }
                if recheck_gap {
                    let check_adapter = adapter.clone();
                    let check_decl = declaration.clone();
                    if let Ok(Ok(Some(candidates))) =
                        crate::async_runtime::spawn_blocking(move || {
                            check_adapter.enumerate_inputs(&check_decl)
                        })
                        .await
                    {
                        if matches!(
                            resolve_candidate(&declaration, &persisted, &candidates),
                            CandidateResolution::Exact(_) | CandidateResolution::Rebound(_)
                        ) {
                            let _ = self.schedule_local_recording_reconcile(
                                plugin_id.to_string(),
                                adapter,
                                ReconcileMode::AttachOrWatch,
                            );
                        }
                    }
                }
            }
            Err(error) => {
                log::debug!(
                    "[local-recording] arrival watch unavailable for {plugin_id}: {}",
                    error.reason
                );
                let teardown = error.teardown;
                let waiters =
                    finish_failed_start(&hotplug, plugin_id, generation, teardown.clone());
                for waiter in waiters {
                    let _ = waiter.send(());
                }
                if let Some(teardown) = teardown {
                    spawn_watcher_reaper(hotplug, plugin_id.to_string(), generation, teardown);
                }
            }
        }
    }

    fn is_eligible_for_auto_attach(&self, plugin_id: &str) -> bool {
        self.local_recording_status(plugin_id).is_ok_and(|status| {
            status.granted
                && status.enabled
                && status.devices_selected
                && !status.available
                && !status.capture_active
        })
    }

    fn eligible_attach_inputs(
        &self,
        plugin_id: &str,
    ) -> Option<(LocalRecordingTrigger, SelectedDevices, u64)> {
        if !self.is_eligible_for_auto_attach(plugin_id) {
            return None;
        }
        let registry = self.ext::<LocalRecordingRegistryHandle>();
        if registry.has_pending_reservation(plugin_id) {
            return None;
        }
        let declaration = registry.declaration(plugin_id)?;
        let (devices, generation) = registry.selected_devices(plugin_id)?;
        Some((declaration, devices, generation))
    }

    fn emit_local_recording_status(&self, plugin_id: &str) {
        if let Ok(status) = self.local_recording_status(plugin_id) {
            self.ui.emit(
                "local-recording-status",
                serde_json::to_value(status).unwrap_or_default(),
            );
        }
    }
}

fn finish_failed_start(
    hotplug: &Arc<LocalRecordingHotplug>,
    plugin_id: &str,
    generation: u64,
    teardown: Option<SubscriptionTeardown>,
) -> Vec<Sender<()>> {
    let mut state = hotplug.state.lock();
    let Some(entry) = state.watchers.get_mut(plugin_id) else {
        return Vec::new();
    };
    if entry.generation != generation {
        return Vec::new();
    }
    match teardown {
        Some(teardown) => {
            let prev = std::mem::replace(&mut entry.phase, WatcherPhase::Stopping(teardown));
            match prev {
                WatcherPhase::Starting { done_waiters, .. } => done_waiters,
                _ => Vec::new(),
            }
        }
        None => match state.watchers.remove(plugin_id) {
            Some(WatcherEntry {
                phase: WatcherPhase::Starting { done_waiters, .. },
                ..
            }) => done_waiters,
            _ => Vec::new(),
        },
    }
}

async fn stop_watcher_async(
    hotplug: Arc<LocalRecordingHotplug>,
    plugin_id: &str,
) -> Result<(), &'static str> {
    let id = plugin_id.to_string();
    crate::async_runtime::spawn_blocking(move || {
        hotplug.stop_watcher_blocking(&id, CLEANUP_DEADLINE)
    })
    .await
    .map_err(|_| "arrival_watch_cleanup_worker_failed")?
}

#[cfg(test)]
mod tests;
