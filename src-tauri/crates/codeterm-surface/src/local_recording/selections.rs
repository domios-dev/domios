use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use codeterm_plugin::local_recording::SelectedDevices;
use parking_lot::Mutex;

use codeterm_util::persisted_state::{Loaded, StateFile};

type Selections = BTreeMap<String, SelectedDevices>;
type EnabledPreferences = BTreeMap<String, bool>;

pub struct RecordingSelectionStore {
    file: StateFile,
    selections: Mutex<Selections>,
    readable: bool,
    enabled_file: StateFile,
    enabled: Mutex<EnabledPreferences>,
    enabled_readable: bool,
}

fn parse(raw: &str) -> Result<Selections, String> {
    let selections: Selections = serde_json::from_str(raw).map_err(|error| error.to_string())?;
    if selections.values().any(|devices| !devices.valid()) {
        return Err("invalid_device_selection".into());
    }
    Ok(selections)
}

fn parse_enabled(raw: &str) -> Result<EnabledPreferences, String> {
    serde_json::from_str(raw).map_err(|error| error.to_string())
}

fn enabled_path(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("local-recording");
    let name = if let Some(prefix) = stem.strip_suffix("-selections") {
        format!("{prefix}-enabled.json")
    } else {
        format!("{stem}-enabled.json")
    };
    path.with_file_name(name)
}

impl RecordingSelectionStore {
    pub fn open(path: PathBuf) -> Self {
        let enabled_store_path = enabled_path(&path);
        let file = StateFile::new(path)
            .with_private_permissions()
            .with_validator(|raw| parse(raw).map(|_| ()));
        let (selections, readable) = match file.load(parse) {
            Loaded::Parsed(selections)
            | Loaded::Recovered {
                value: selections, ..
            } => (selections, true),
            Loaded::Missing => (Selections::new(), file.held().is_none()),
            Loaded::Unrecoverable { .. } => (Selections::new(), false),
        };
        let enabled_file = StateFile::new(enabled_store_path)
            .with_private_permissions()
            .with_validator(|raw| parse_enabled(raw).map(|_| ()));
        let (enabled, enabled_readable) = match enabled_file.load(parse_enabled) {
            Loaded::Parsed(enabled) | Loaded::Recovered { value: enabled, .. } => (enabled, true),
            Loaded::Missing => (EnabledPreferences::new(), enabled_file.held().is_none()),
            Loaded::Unrecoverable { .. } => (EnabledPreferences::new(), false),
        };
        Self {
            file,
            selections: Mutex::new(selections),
            readable,
            enabled_file,
            enabled: Mutex::new(enabled),
            enabled_readable,
        }
    }

    pub fn get(&self, plugin_id: &str) -> Result<Option<SelectedDevices>, String> {
        if !self.readable {
            return Err("recording_selection_store_unavailable".into());
        }
        Ok(self.selections.lock().get(plugin_id).cloned())
    }

    pub fn set(&self, plugin_id: &str, devices: &SelectedDevices) -> Result<(), String> {
        if !self.readable {
            return Err("recording_selection_store_unavailable".into());
        }
        if !devices.valid() {
            return Err("invalid_device_selection".into());
        }
        let mut selections = self.selections.lock();
        let mut next = selections.clone();
        next.insert(plugin_id.into(), devices.clone());
        let json = serde_json::to_string(&next).map_err(|error| error.to_string())?;
        self.file
            .save(&json)
            .map_err(|_| "recording_selection_persist_failed".to_string())?;
        *selections = next;
        Ok(())
    }

    pub fn enabled(&self, plugin_id: &str) -> Result<bool, String> {
        if !self.enabled_readable {
            return Err("recording_preference_store_unavailable".into());
        }
        Ok(self.enabled.lock().get(plugin_id).copied().unwrap_or(false))
    }

    pub fn set_enabled(&self, plugin_id: &str, enabled: bool) -> Result<(), String> {
        if !self.enabled_readable {
            return Err("recording_preference_store_unavailable".into());
        }
        let mut prefs = self.enabled.lock();
        let mut next = prefs.clone();
        next.insert(plugin_id.into(), enabled);
        let json = serde_json::to_string(&next).map_err(|error| error.to_string())?;
        self.enabled_file
            .save(&json)
            .map_err(|_| "recording_preference_persist_failed".to_string())?;
        *prefs = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
