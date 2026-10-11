use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

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

pub struct SubscriptionTeardown(std::sync::mpsc::Receiver<Result<(), String>>);

impl SubscriptionTeardown {
    pub fn new(completion: std::sync::mpsc::Receiver<Result<(), String>>) -> Self {
        Self(completion)
    }
    pub fn wait(self) -> Result<(), String> {
        self.0
            .recv()
            .map_err(|_| "subscription_teardown_unacknowledged".to_string())?
    }
}

fn retire(subscription: &mut dyn LocalInputSubscription) {
    let completion = subscription.request_stop();
    std::thread::spawn(move || match completion.wait() {
        Ok(()) => log::debug!("[local-recording] subscription teardown acknowledged"),
        Err(error) => log::debug!("[local-recording] teardown failed: {error}"),
    });
}

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

struct Entry {
    declaration: LocalRecordingTrigger,
    granted: bool,
    enabled: bool,
    devices: Option<SelectedDevices>,
    available: bool,
    generation: u64,
    active: Option<Arc<AtomicBool>>,
    subscription: Option<Box<dyn LocalInputSubscription>>,
}

#[derive(Default)]
pub struct LocalRecordingRegistry {
    entries: Mutex<BTreeMap<String, Entry>>,
}

pub type LocalRecordingRegistryHandle = Arc<LocalRecordingRegistry>;

impl LocalRecordingRegistry {
    pub fn register(&self, id: &str, declaration: LocalRecordingTrigger, granted: bool) {
        self.unregister(id);
        self.entries.lock().insert(
            id.into(),
            Entry {
                declaration,
                granted,
                enabled: false,
                devices: None,
                available: false,
                generation: next_generation(),
                active: None,
                subscription: None,
            },
        );
    }

    pub fn unregister(&self, id: &str) {
        if let Some(mut entry) = self.entries.lock().remove(id) {
            invalidate(&mut entry);
        }
    }

    pub fn contains(&self, id: &str) -> bool {
        self.entries.lock().contains_key(id)
    }

    pub fn declaration(&self, id: &str) -> Option<LocalRecordingTrigger> {
        self.entries
            .lock()
            .get(id)
            .map(|entry| entry.declaration.clone())
    }

    pub fn status(&self, id: &str) -> Result<RecordingStatus, &'static str> {
        let entries = self.entries.lock();
        let entry = entries.get(id).ok_or("trigger_not_declared")?;
        Ok(status(id, entry))
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<RecordingStatus, &'static str> {
        let mut entries = self.entries.lock();
        let entry = entries.get_mut(id).ok_or("trigger_not_declared")?;
        if enabled && !entry.granted {
            return Err("local_recording_grant_required");
        }
        if entry.enabled != enabled {
            invalidate(entry);
            entry.enabled = enabled;
        }
        Ok(status(id, entry))
    }

    pub fn revoke(&self, id: &str) {
        if let Some(entry) = self.entries.lock().get_mut(id) {
            invalidate(entry);
            entry.granted = false;
            entry.enabled = false;
        }
    }

    pub fn select_devices(&self, id: &str, devices: SelectedDevices) -> Result<u64, &'static str> {
        if devices.input_id.is_empty()
            || devices.collection_id.is_empty()
            || devices.microphone_id.is_empty()
        {
            return Err("devices_unavailable");
        }
        let mut entries = self.entries.lock();
        let entry = entries.get_mut(id).ok_or("trigger_not_declared")?;
        invalidate(entry);
        entry.devices = Some(devices);
        entry.available = false;
        Ok(entry.generation)
    }

    pub fn selected_devices(&self, id: &str) -> Option<(SelectedDevices, u64)> {
        let entries = self.entries.lock();
        let entry = entries.get(id)?;
        Some((entry.devices.clone()?, entry.generation))
    }

    pub fn attach_subscription(
        &self,
        id: &str,
        generation: u64,
        mut subscription: Box<dyn LocalInputSubscription>,
    ) -> Result<(), &'static str> {
        let mut entries = self.entries.lock();
        let valid = entries.get_mut(id).filter(|entry| {
            entry.granted
                && entry.enabled
                && entry.generation == generation
                && entry.devices.is_some()
        });
        let Some(entry) = valid else {
            retire(subscription.as_mut());
            return Err("subscription_stale");
        };
        if let Some(mut previous) = entry.subscription.replace(subscription) {
            retire(previous.as_mut());
        }
        Ok(())
    }

    pub fn set_available(&self, id: &str, generation: u64, available: bool) -> bool {
        let mut entries = self.entries.lock();
        let Some(entry) = entries.get_mut(id) else {
            return false;
        };
        if entry.generation != generation {
            return false;
        }
        if !available {
            invalidate(entry);
        }
        entry.available = available;
        true
    }

    pub fn begin(
        &self,
        id: &str,
        devices: &SelectedDevices,
        generation: u64,
    ) -> Result<Arc<AtomicBool>, &'static str> {
        let mut entries = self.entries.lock();
        if entries.values().any(|entry| {
            entry
                .active
                .as_ref()
                .is_some_and(|active| active.load(Ordering::Acquire))
        }) {
            return Err("parallel_capture");
        }
        let entry = entries.get_mut(id).ok_or("trigger_not_declared")?;
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
        if let Some(entry) = self.entries.lock().get_mut(id) {
            invalidate(entry);
        }
    }
}

fn invalidate(entry: &mut Entry) {
    entry.available = false;
    if let Some(active) = entry.active.take() {
        active.store(false, Ordering::Release);
    }
    entry.generation = next_generation();
    if let Some(mut subscription) = entry.subscription.take() {
        retire(subscription.as_mut());
    }
}

fn next_generation() -> u64 {
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
