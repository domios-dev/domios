use std::{collections::BTreeMap, path::PathBuf};

use codeterm_plugin::local_recording::SelectedDevices;
use parking_lot::Mutex;

use codeterm_util::persisted_state::{Loaded, StateFile};

type Selections = BTreeMap<String, SelectedDevices>;

pub struct RecordingSelectionStore {
    file: StateFile,
    selections: Mutex<Selections>,
    readable: bool,
}

fn parse(raw: &str) -> Result<Selections, String> {
    let selections: Selections = serde_json::from_str(raw).map_err(|error| error.to_string())?;
    if selections.values().any(|devices| !devices.valid()) {
        return Err("invalid_device_selection".into());
    }
    Ok(selections)
}

impl RecordingSelectionStore {
    pub fn open(path: PathBuf) -> Self {
        let file = StateFile::new(path).with_validator(|raw| parse(raw).map(|_| ()));
        let (selections, readable) = match file.load(parse) {
            Loaded::Parsed(selections)
            | Loaded::Recovered {
                value: selections, ..
            } => (selections, true),
            Loaded::Missing => (Selections::new(), file.held().is_none()),
            Loaded::Unrecoverable { .. } => (Selections::new(), false),
        };
        Self {
            file,
            selections: Mutex::new(selections),
            readable,
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
}

#[cfg(test)]
mod tests;
