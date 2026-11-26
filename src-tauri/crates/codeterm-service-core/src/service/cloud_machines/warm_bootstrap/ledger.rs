use std::collections::BTreeMap;
use std::path::PathBuf;

use codeterm_util::persisted_state::{Loaded, StateFile};
use serde::{Deserialize, Serialize};

use super::types::{CloudCreatePhases, CloudWarmBootstrapSummary};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MachineWarmBootstrapEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creation_generation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub create_phases: Option<CloudCreatePhases>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warm_bootstrap: Option<CloudWarmBootstrapSummary>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct PersistedWarmLedger {
    schema_version: u32,
    entries: BTreeMap<String, MachineWarmBootstrapEntry>,
}

pub struct CloudWarmLedgerStore {
    path: PathBuf,
}

impl CloudWarmLedgerStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn beside_machine_store(machine_store_path: &std::path::Path) -> Self {
        Self::new(machine_store_path.with_file_name("cloud_warm_bootstrap_ledger.json"))
    }

    fn state_file(&self) -> StateFile {
        StateFile::new(&self.path).with_validator(|text| {
            serde_json::from_str::<PersistedWarmLedger>(text)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    pub fn load(&self) -> BTreeMap<String, MachineWarmBootstrapEntry> {
        match self.state_file().load(|text| {
            serde_json::from_str::<PersistedWarmLedger>(text).map_err(|error| error.to_string())
        }) {
            Loaded::Parsed(data) | Loaded::Recovered { value: data, .. } => data.entries,
            Loaded::Missing | Loaded::Unrecoverable { .. } => BTreeMap::new(),
        }
    }

    pub fn save(
        &self,
        entries: &BTreeMap<String, MachineWarmBootstrapEntry>,
    ) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let payload = PersistedWarmLedger {
            schema_version: 1,
            entries: entries.clone(),
        };
        let encoded = serde_json::to_string_pretty(&payload).map_err(|error| error.to_string())?;
        self.state_file().save(&encoded)
    }
}
