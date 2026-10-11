use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

const MAX_NATIVE_RESERVATIONS: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct LocalRecordingTrigger {
    pub vendor_id: u16,
    pub product_id: u16,
    pub usage_page: u16,
    pub usage: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectedDevices {
    pub input_id: String,
    pub collection_id: String,
    pub microphone_id: String,
}

pub trait LocalInputSubscription: Send {
    fn request_stop(&mut self) -> SubscriptionTeardown;
}

#[derive(Clone)]
pub struct SubscriptionTeardown(Arc<Mutex<SubscriptionTeardownSource>>);

enum SubscriptionTeardownSource {
    Receiver(std::sync::mpsc::Receiver<Result<(), String>>),
    Waiter(Box<dyn FnMut(Duration) -> SubscriptionTeardownWait + Send>),
}

impl SubscriptionTeardown {
    pub fn new(completion: std::sync::mpsc::Receiver<Result<(), String>>) -> Self {
        Self(Arc::new(Mutex::new(SubscriptionTeardownSource::Receiver(
            completion,
        ))))
    }

    pub fn with_waiter(
        waiter: impl FnMut(Duration) -> SubscriptionTeardownWait + Send + 'static,
    ) -> Self {
        Self(Arc::new(Mutex::new(SubscriptionTeardownSource::Waiter(
            Box::new(waiter),
        ))))
    }

    pub fn wait_timeout(&self, timeout: Duration) -> SubscriptionTeardownWait {
        let Some(mut source) = self.0.try_lock() else {
            return SubscriptionTeardownWait::Pending;
        };
        match &mut *source {
            SubscriptionTeardownSource::Receiver(receiver) => {
                match receiver.recv_timeout(timeout) {
                    Ok(Ok(())) => SubscriptionTeardownWait::Complete,
                    Ok(Err(error)) => SubscriptionTeardownWait::Failed(error),
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        SubscriptionTeardownWait::Pending
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        SubscriptionTeardownWait::Failed(
                            "subscription_teardown_unacknowledged".into(),
                        )
                    }
                }
            }
            SubscriptionTeardownSource::Waiter(waiter) => waiter(timeout),
        }
    }
}

pub enum SubscriptionTeardownWait {
    Complete,
    Pending,
    Failed(String),
}

pub struct LocalInputSetupError {
    pub reason: String,
    pub teardown: Option<SubscriptionTeardown>,
}

#[derive(Clone, Debug)]
pub struct LocalInputSetup {
    pub declaration: LocalRecordingTrigger,
    pub devices: SelectedDevices,
    pub generation: u64,
    plugin_id: String,
    reservation_id: u64,
}

enum ReservationPhase {
    SettingUp { cancelled: bool },
    Active(Box<dyn LocalInputSubscription>),
    StopRequested,
    Stopping(SubscriptionTeardown),
}

struct NativeReservation {
    plugin_id: String,
    devices: SelectedDevices,
    generation: u64,
    phase: ReservationPhase,
}

struct Entry {
    declaration: LocalRecordingTrigger,
    granted: bool,
    enabled: bool,
    devices: Option<SelectedDevices>,
    available: bool,
    generation: u64,
    active: Option<Arc<AtomicBool>>,
    reservation_id: Option<u64>,
}

#[derive(Default)]
struct RegistryState {
    entries: BTreeMap<String, Entry>,
    reservations: BTreeMap<u64, NativeReservation>,
}

#[derive(Default)]
pub struct LocalRecordingRegistry {
    state: Mutex<RegistryState>,
}

pub type LocalRecordingRegistryHandle = Arc<LocalRecordingRegistry>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingStatus {
    pub plugin_id: String,
    pub granted: bool,
    pub enabled: bool,
    pub devices_selected: bool,
    pub available: bool,
    pub capture_active: bool,
    pub generation: u64,
}

impl LocalRecordingRegistry {
    pub fn register(&self, id: &str, declaration: LocalRecordingTrigger, granted: bool) {
        self.unregister(id);
        self.state.lock().entries.insert(
            id.into(),
            Entry {
                declaration,
                granted,
                enabled: false,
                devices: None,
                available: false,
                generation: next_generation(),
                active: None,
                reservation_id: None,
            },
        );
    }

    pub fn unregister(&self, id: &str) {
        let stopped = {
            let mut state = self.state.lock();
            let Some(mut entry) = state.entries.remove(id) else {
                return;
            };
            let RegistryState { reservations, .. } = &mut *state;
            invalidate_entry(&mut entry, reservations)
        };
        if let Some((reservation_id, subscription)) = stopped {
            self.request_stop(reservation_id, subscription);
        }
    }

    pub fn contains(&self, id: &str) -> bool {
        self.state.lock().entries.contains_key(id)
    }

    pub fn declaration(&self, id: &str) -> Option<LocalRecordingTrigger> {
        self.state
            .lock()
            .entries
            .get(id)
            .map(|entry| entry.declaration.clone())
    }

    pub fn status(&self, id: &str) -> Result<RecordingStatus, &'static str> {
        let state = self.state.lock();
        let entry = state.entries.get(id).ok_or("trigger_not_declared")?;
        Ok(status(id, entry))
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<RecordingStatus, &'static str> {
        let (result, stopped) = {
            let mut state = self.state.lock();
            let RegistryState {
                entries,
                reservations,
            } = &mut *state;
            let entry = entries.get_mut(id).ok_or("trigger_not_declared")?;
            if enabled && !entry.granted {
                return Err("local_recording_grant_required");
            }
            let stopped = if entry.enabled != enabled {
                let stopped = invalidate_entry(entry, reservations);
                entry.enabled = enabled;
                stopped
            } else {
                None
            };
            (status(id, entry), stopped)
        };
        if let Some((reservation_id, subscription)) = stopped {
            self.request_stop(reservation_id, subscription);
        }
        Ok(result)
    }

    pub fn revoke(&self, id: &str) {
        let stopped = {
            let mut state = self.state.lock();
            let RegistryState {
                entries,
                reservations,
            } = &mut *state;
            let Some(entry) = entries.get_mut(id) else {
                return;
            };
            let stopped = invalidate_entry(entry, reservations);
            entry.granted = false;
            entry.enabled = false;
            stopped
        };
        if let Some((reservation_id, subscription)) = stopped {
            self.request_stop(reservation_id, subscription);
        }
    }

    pub fn select_devices(&self, id: &str, devices: SelectedDevices) -> Result<u64, &'static str> {
        if devices.input_id.is_empty()
            || devices.collection_id.is_empty()
            || devices.microphone_id.is_empty()
        {
            return Err("devices_unavailable");
        }
        let (generation, stopped) = {
            let mut state = self.state.lock();
            let RegistryState {
                entries,
                reservations,
            } = &mut *state;
            let entry = entries.get_mut(id).ok_or("trigger_not_declared")?;
            let stopped = invalidate_entry(entry, reservations);
            entry.devices = Some(devices);
            entry.available = false;
            (entry.generation, stopped)
        };
        if let Some((reservation_id, subscription)) = stopped {
            self.request_stop(reservation_id, subscription);
        }
        Ok(generation)
    }

    pub fn selected_devices(&self, id: &str) -> Option<(SelectedDevices, u64)> {
        let state = self.state.lock();
        let entry = state.entries.get(id)?;
        Some((entry.devices.clone()?, entry.generation))
    }

    pub fn begin_subscription_setup(&self, id: &str) -> Result<LocalInputSetup, &'static str> {
        let mut state = self.state.lock();
        let (declaration, devices, generation) = {
            let entry = state.entries.get(id).ok_or("trigger_not_declared")?;
            if !entry.granted {
                return Err("local_recording_grant_required");
            }
            if !entry.enabled {
                return Err("trigger_disabled");
            }
            let devices = entry.devices.clone().ok_or("devices_unavailable")?;
            (entry.declaration.clone(), devices, entry.generation)
        };
        if state
            .reservations
            .values()
            .any(|reservation| reservations_conflict(id, &devices, reservation))
        {
            return Err("subscription_cleanup_pending");
        }
        if state.reservations.len() >= MAX_NATIVE_RESERVATIONS {
            return Err("subscription_reservation_limit");
        }
        let reservation_id = next_reservation_id();
        state.reservations.insert(
            reservation_id,
            NativeReservation {
                plugin_id: id.into(),
                devices: devices.clone(),
                generation,
                phase: ReservationPhase::SettingUp { cancelled: false },
            },
        );
        state
            .entries
            .get_mut(id)
            .expect("validated entry")
            .reservation_id = Some(reservation_id);
        Ok(LocalInputSetup {
            declaration,
            devices,
            generation,
            plugin_id: id.into(),
            reservation_id,
        })
    }

    pub fn complete_subscription_setup(
        &self,
        setup: &LocalInputSetup,
        subscription: Box<dyn LocalInputSubscription>,
    ) -> Result<(), &'static str> {
        {
            let mut state = self.state.lock();
            let valid = state
                .reservations
                .get(&setup.reservation_id)
                .is_some_and(|reservation| {
                    matches!(
                        reservation.phase,
                        ReservationPhase::SettingUp { cancelled: false }
                    ) && reservation.plugin_id == setup.plugin_id
                        && reservation.devices == setup.devices
                        && reservation.generation == setup.generation
                })
                && state.entries.get(&setup.plugin_id).is_some_and(|entry| {
                    entry.granted
                        && entry.enabled
                        && entry.devices.as_ref() == Some(&setup.devices)
                        && entry.generation == setup.generation
                        && entry.reservation_id == Some(setup.reservation_id)
                });
            if valid {
                state
                    .reservations
                    .get_mut(&setup.reservation_id)
                    .unwrap()
                    .phase = ReservationPhase::Active(subscription);
                state.entries.get_mut(&setup.plugin_id).unwrap().available = true;
                return Ok(());
            }
            if let Some(reservation) = state.reservations.get_mut(&setup.reservation_id) {
                reservation.phase = ReservationPhase::StopRequested;
            } else {
                state.reservations.insert(
                    setup.reservation_id,
                    NativeReservation {
                        plugin_id: setup.plugin_id.clone(),
                        devices: setup.devices.clone(),
                        generation: setup.generation,
                        phase: ReservationPhase::StopRequested,
                    },
                );
            }
        }
        self.request_stop(setup.reservation_id, subscription);
        Err("subscription_stale")
    }

    pub fn fail_subscription_setup(
        &self,
        setup: &LocalInputSetup,
        error: LocalInputSetupError,
    ) -> String {
        let mut state = self.state.lock();
        if let Some(entry) = state.entries.get_mut(&setup.plugin_id) {
            if entry.reservation_id == Some(setup.reservation_id) {
                entry.reservation_id = None;
                entry.available = false;
            }
        }
        match error.teardown {
            Some(teardown) => {
                if let Some(reservation) = state.reservations.get_mut(&setup.reservation_id) {
                    reservation.phase = ReservationPhase::Stopping(teardown);
                } else {
                    state.reservations.insert(
                        setup.reservation_id,
                        NativeReservation {
                            plugin_id: setup.plugin_id.clone(),
                            devices: setup.devices.clone(),
                            generation: setup.generation,
                            phase: ReservationPhase::Stopping(teardown),
                        },
                    );
                }
            }
            None => {
                state.reservations.remove(&setup.reservation_id);
            }
        }
        error.reason
    }

    pub fn cancel_subscription_setup(&self, setup: &LocalInputSetup) {
        let stopped = {
            let mut state = self.state.lock();
            let RegistryState {
                entries,
                reservations,
            } = &mut *state;
            let stopped = if let Some(entry) = entries.get_mut(&setup.plugin_id) {
                if entry.reservation_id == Some(setup.reservation_id) {
                    invalidate_entry(entry, reservations)
                } else {
                    None
                }
            } else {
                mark_setup_cancelled(reservations, setup.reservation_id);
                None
            };
            if stopped.is_none() {
                mark_setup_cancelled(reservations, setup.reservation_id);
            }
            stopped
        };
        if let Some((reservation_id, subscription)) = stopped {
            self.request_stop(reservation_id, subscription);
        }
    }

    pub fn wait_for_subscription_slot(
        &self,
        id: &str,
        timeout: Duration,
    ) -> Result<(), &'static str> {
        self.reap_completed();
        let devices = self
            .state
            .lock()
            .entries
            .get(id)
            .and_then(|entry| entry.devices.clone());
        let conflicts = {
            let state = self.state.lock();
            state
                .reservations
                .iter()
                .filter(|(_, reservation)| {
                    devices
                        .as_ref()
                        .is_some_and(|devices| reservations_conflict(id, devices, reservation))
                        || reservation.plugin_id == id
                })
                .map(|(reservation_id, reservation)| {
                    let handle = match &reservation.phase {
                        ReservationPhase::Stopping(handle) => Some(handle.clone()),
                        _ => None,
                    };
                    (*reservation_id, handle)
                })
                .collect::<Vec<_>>()
        };
        let deadline = Instant::now() + timeout;
        for (reservation_id, handle) in conflicts {
            let Some(handle) = handle else {
                return Err("subscription_cleanup_pending");
            };
            let remaining = deadline.saturating_duration_since(Instant::now());
            match handle.wait_timeout(remaining) {
                SubscriptionTeardownWait::Complete => {
                    self.state.lock().reservations.remove(&reservation_id);
                }
                SubscriptionTeardownWait::Pending => {
                    return Err("subscription_cleanup_pending");
                }
                SubscriptionTeardownWait::Failed(error) => {
                    log::debug!("[local-recording] teardown still owns reservation: {error}");
                    return Err("subscription_cleanup_pending");
                }
            }
        }
        Ok(())
    }

    pub fn mark_unavailable(&self, id: &str, generation: u64) -> bool {
        let stopped = {
            let mut state = self.state.lock();
            let RegistryState {
                entries,
                reservations,
            } = &mut *state;
            let Some(entry) = entries.get_mut(id) else {
                return false;
            };
            if entry.generation != generation {
                return false;
            }
            invalidate_entry(entry, reservations)
        };
        if let Some((reservation_id, subscription)) = stopped {
            self.request_stop(reservation_id, subscription);
        }
        true
    }

    pub fn begin(
        &self,
        id: &str,
        devices: &SelectedDevices,
        generation: u64,
    ) -> Result<Arc<AtomicBool>, &'static str> {
        let mut state = self.state.lock();
        if state.entries.values().any(|entry| {
            entry
                .active
                .as_ref()
                .is_some_and(|active| active.load(Ordering::Acquire))
        }) {
            return Err("parallel_capture");
        }
        let entry = state.entries.get_mut(id).ok_or("trigger_not_declared")?;
        if !entry.granted {
            return Err("local_recording_grant_required");
        }
        if !entry.enabled {
            return Err("trigger_disabled");
        }
        if !entry.available
            || entry.devices.as_ref() != Some(devices)
            || entry.generation != generation
        {
            return Err("devices_unavailable");
        }
        let active = Arc::new(AtomicBool::new(true));
        entry.active = Some(active.clone());
        Ok(active)
    }

    pub fn cancel(&self, id: &str) {
        let stopped = {
            let mut state = self.state.lock();
            let RegistryState {
                entries,
                reservations,
            } = &mut *state;
            let Some(entry) = entries.get_mut(id) else {
                return;
            };
            invalidate_entry(entry, reservations)
        };
        if let Some((reservation_id, subscription)) = stopped {
            self.request_stop(reservation_id, subscription);
        }
    }

    fn request_stop(&self, reservation_id: u64, mut subscription: Box<dyn LocalInputSubscription>) {
        let teardown = subscription.request_stop();
        let mut state = self.state.lock();
        if let Some(reservation) = state.reservations.get_mut(&reservation_id) {
            reservation.phase = ReservationPhase::Stopping(teardown);
        }
    }

    fn reap_completed(&self) {
        let pending = {
            let state = self.state.lock();
            state
                .reservations
                .iter()
                .filter_map(|(id, reservation)| match &reservation.phase {
                    ReservationPhase::Stopping(handle) => Some((*id, handle.clone())),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        let completed = pending
            .into_iter()
            .filter_map(|(id, handle)| {
                matches!(
                    handle.wait_timeout(Duration::ZERO),
                    SubscriptionTeardownWait::Complete
                )
                .then_some(id)
            })
            .collect::<Vec<_>>();
        let mut state = self.state.lock();
        for id in completed {
            state.reservations.remove(&id);
        }
    }
}

fn invalidate_entry(
    entry: &mut Entry,
    reservations: &mut BTreeMap<u64, NativeReservation>,
) -> Option<(u64, Box<dyn LocalInputSubscription>)> {
    entry.available = false;
    if let Some(active) = entry.active.take() {
        active.store(false, Ordering::Release);
    }
    entry.generation = next_generation();
    let reservation_id = entry.reservation_id.take()?;
    let reservation = reservations.get_mut(&reservation_id)?;
    match std::mem::replace(&mut reservation.phase, ReservationPhase::StopRequested) {
        ReservationPhase::Active(subscription) => Some((reservation_id, subscription)),
        ReservationPhase::SettingUp { .. } => {
            reservation.phase = ReservationPhase::SettingUp { cancelled: true };
            None
        }
        ReservationPhase::StopRequested => None,
        ReservationPhase::Stopping(teardown) => {
            reservation.phase = ReservationPhase::Stopping(teardown);
            None
        }
    }
}

fn mark_setup_cancelled(reservations: &mut BTreeMap<u64, NativeReservation>, id: u64) {
    let Some(reservation) = reservations.get_mut(&id) else {
        return;
    };
    if matches!(reservation.phase, ReservationPhase::SettingUp { .. }) {
        reservation.phase = ReservationPhase::SettingUp { cancelled: true };
    }
}

fn reservations_conflict(
    id: &str,
    devices: &SelectedDevices,
    existing: &NativeReservation,
) -> bool {
    existing.plugin_id == id
        || (existing.devices.input_id == devices.input_id
            && existing.devices.collection_id == devices.collection_id)
}

fn next_generation() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn next_reservation_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

fn status(id: &str, entry: &Entry) -> RecordingStatus {
    RecordingStatus {
        plugin_id: id.into(),
        granted: entry.granted,
        enabled: entry.enabled,
        devices_selected: entry.devices.is_some(),
        available: entry.available,
        capture_active: entry
            .active
            .as_ref()
            .is_some_and(|active| active.load(Ordering::Acquire)),
        generation: entry.generation,
    }
}

#[cfg(test)]
mod tests;
