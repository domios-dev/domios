//! Host-owned binding from a webhook-receiving plugin to the one tab that receives its events.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use serde::Serialize;

use codeterm_plugin::tab_menu::TabMenuAction;
use codeterm_util::persisted_state::{Loaded, StateFile};

pub const RECEIVER_TARGET_CONTRIBUTION_KIND: &str = "receiver_target";

type Bindings = BTreeMap<String, String>;

/// Sole owner of the webhook-receiver → tab bindings.
pub struct ReceiverTargets(pub std::sync::Arc<ReceiverTargetStore>);

pub struct LocalRecordingTargets(pub std::sync::Arc<ReceiverTargetStore>);

pub struct ReceiverTargetStore {
    path: PathBuf,
    bindings: Mutex<Bindings>,
    readable: bool,
}

impl ReceiverTargetStore {
    pub fn default_path() -> PathBuf {
        codeterm_paths::data_dir().join("receiver-targets.json")
    }

    pub fn open(path: PathBuf) -> Self {
        let (bindings, readable) = load(&path);
        Self {
            path,
            bindings: Mutex::new(bindings),
            readable,
        }
    }

    pub fn get(&self, plugin_id: &str) -> Option<String> {
        self.bindings.lock().get(plugin_id).cloned()
    }

    pub fn readable(&self) -> bool {
        self.readable
    }

    pub fn bindings(&self) -> BTreeMap<String, String> {
        self.bindings.lock().clone()
    }

    /// One tab per receiver: binding a new tab replaces the previous one.
    pub fn set(&self, plugin_id: &str, tab_id: &str) -> Result<bool, String> {
        self.mutate(|bindings| {
            bindings
                .insert(plugin_id.to_string(), tab_id.to_string())
                .as_deref()
                != Some(tab_id)
        })
    }

    pub fn clear(&self, plugin_id: &str) -> Result<bool, String> {
        self.mutate(|bindings| bindings.remove(plugin_id).is_some())
    }

    pub fn forget_tab(&self, tab_id: &str) -> Result<bool, String> {
        self.mutate(|bindings| {
            let before = bindings.len();
            bindings.retain(|_, bound| bound != tab_id);
            bindings.len() != before
        })
    }

    /// Disk is written before memory, so a failed write never reports a binding a restart would lose.
    fn mutate(&self, change: impl FnOnce(&mut Bindings) -> bool) -> Result<bool, String> {
        let mut bindings = self.bindings.lock();
        let mut next = bindings.clone();
        if !change(&mut next) {
            return Ok(false);
        }
        persist(&self.path, &next)?;
        *bindings = next;
        Ok(true)
    }
}

fn state_file(path: &Path) -> StateFile {
    StateFile::new(path).with_validator(validate)
}

fn parse(raw: &str) -> Result<Bindings, String> {
    serde_json::from_str(raw).map_err(|error| error.to_string())
}

fn validate(raw: &str) -> Result<(), String> {
    parse(raw).map(|_| ())
}

/// A held file leaves every receiver on the General Agent path and refuses
/// every binding change until it loads again.
fn load(path: &Path) -> (Bindings, bool) {
    match state_file(path).load(parse) {
        Loaded::Parsed(bindings)
        | Loaded::Recovered {
            value: bindings, ..
        } => (bindings, true),
        Loaded::Missing => (Bindings::new(), state_file(path).held().is_none()),
        Loaded::Unrecoverable { .. } => (Bindings::new(), false),
    }
}

fn persist(path: &Path, bindings: &Bindings) -> Result<(), String> {
    let json = serde_json::to_string_pretty(bindings).map_err(|error| error.to_string())?;
    state_file(path).save(&json).map_err(|error| {
        format!(
            "could not record receiver targets at {}: {error}",
            path.display()
        )
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetLiveness {
    Live,
    NotLive,
    Absent,
}

/// The bound tab only while it can receive; `None` keeps the General Agent path.
pub fn live_target(
    bound: Option<String>,
    liveness: impl FnOnce(&str) -> TargetLiveness,
) -> Option<String> {
    bound.filter(|tab_id| liveness(tab_id) == TargetLiveness::Live)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiverTargetView {
    pub plugin_id: String,
    pub tab_id: Option<String>,
    pub live: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabMenuEntry {
    pub plugin_id: String,
    pub item_id: String,
    pub label: String,
    pub action: TabMenuAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_svg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon_color: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabMenuState {
    pub items: Vec<TabMenuEntry>,
    pub receiver_targets: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiverTargetError {
    ReceiverNotFound,
    TabNotFound,
    MenuItemNotFound,
    PersistFailed,
}

impl ReceiverTargetError {
    pub fn code(self) -> &'static str {
        match self {
            Self::ReceiverNotFound => "receiver_not_found",
            Self::TabNotFound => "tab_not_found",
            Self::MenuItemNotFound => "menu_item_not_found",
            Self::PersistFailed => "persist_failed",
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::ReceiverNotFound => "No active plugin with this id declares a webhook receiver",
            Self::TabNotFound => "Tab not found on this machine",
            Self::MenuItemNotFound => "The plugin declares no such tab menu item",
            Self::PersistFailed => "The receiver target could not be recorded",
        }
    }
}

pub fn host_json(result: Result<ReceiverTargetView, ReceiverTargetError>) -> String {
    match result {
        Ok(view) => serde_json::to_string(&view).unwrap_or_else(|_| "null".to_string()),
        Err(error) => {
            serde_json::json!({ "error": error.code(), "message": error.message() }).to_string()
        }
    }
}

#[cfg(test)]
mod tests;
