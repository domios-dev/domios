use std::path::{Path, PathBuf};

use codeterm_util::persisted_state::{Loaded, StateFile};
use serde::{Deserialize, Serialize};

use super::held_sends::HeldCloudSend;
use super::types::CloudMachineRecord;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct PersistedCloudMachines {
    schema_version: u32,
    machines: Vec<CloudMachineRecord>,
}

pub struct CloudMachineStore {
    path: PathBuf,
}

impl CloudMachineStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn default_path() -> PathBuf {
        codeterm_paths::data_dir()
            .join("mesh")
            .join("cloud_machines.json")
    }

    pub fn wake_queue_store(&self) -> CloudWakeQueueStore {
        let queue_path = self.path.with_file_name("cloud_wake_queue.json");
        CloudWakeQueueStore::new(queue_path)
    }

    fn state_file(&self) -> StateFile {
        StateFile::new(&self.path).with_validator(|text| {
            serde_json::from_str::<PersistedCloudMachines>(text)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
    }

    pub fn load(&self) -> Vec<CloudMachineRecord> {
        match self.state_file().load(|text| {
            serde_json::from_str::<PersistedCloudMachines>(text).map_err(|error| error.to_string())
        }) {
            Loaded::Parsed(data) | Loaded::Recovered { value: data, .. } => data.machines,
            Loaded::Missing | Loaded::Unrecoverable { .. } => Vec::new(),
        }
    }

    pub fn save(&self, machines: &[CloudMachineRecord]) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let payload = PersistedCloudMachines {
            schema_version: 1,
            machines: machines.to_vec(),
        };
        let encoded = serde_json::to_string_pretty(&payload).map_err(|error| error.to_string())?;
        self.state_file().save(&encoded)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct PersistedCloudWakeQueue {
    schema_version: u32,
    next_sequence: u64,
    sends: Vec<HeldCloudSend>,
}

pub struct CloudWakeQueueStore {
    path: PathBuf,
}

impl CloudWakeQueueStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn state_file(&self) -> StateFile {
        StateFile::new(&self.path).with_validator(|text| {
            serde_json::from_str::<PersistedCloudWakeQueue>(text)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
    }

    pub fn load(&self) -> (u64, Vec<HeldCloudSend>) {
        match self.state_file().load(|text| {
            serde_json::from_str::<PersistedCloudWakeQueue>(text).map_err(|error| error.to_string())
        }) {
            Loaded::Parsed(data) | Loaded::Recovered { value: data, .. } => {
                (data.next_sequence, data.sends)
            }
            Loaded::Missing | Loaded::Unrecoverable { .. } => (0, Vec::new()),
        }
    }

    pub fn save(&self, next_sequence: u64, sends: &[HeldCloudSend]) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let payload = PersistedCloudWakeQueue {
            schema_version: 1,
            next_sequence,
            sends: sends.to_vec(),
        };
        let encoded = serde_json::to_string_pretty(&payload).map_err(|error| error.to_string())?;
        self.state_file().save(&encoded)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
