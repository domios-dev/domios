//! Host-owned binding from a webhook-receiving plugin to the one tab that receives its events.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use parking_lot::Mutex;
use serde::Serialize;

use crate::multiplexer::types::TabBacking;
use crate::plugin::tab_menu::TabMenuAction;
use crate::service::ServiceContext;

pub const RECEIVER_TARGET_CONTRIBUTION_KIND: &str = "receiver_target";

type Bindings = BTreeMap<String, String>;

pub struct ReceiverTargetStore {
    path: PathBuf,
    bindings: Mutex<Bindings>,
}

impl ReceiverTargetStore {
    pub fn default_path() -> PathBuf {
        crate::paths::data_dir().join("receiver-targets.json")
    }

    pub fn open(path: PathBuf) -> Self {
        let bindings = load(&path);
        Self {
            path,
            bindings: Mutex::new(bindings),
        }
    }

    pub fn get(&self, plugin_id: &str) -> Option<String> {
        self.bindings.lock().get(plugin_id).cloned()
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

fn load(path: &Path) -> Bindings {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Bindings::new();
    };
    serde_json::from_str(&raw).unwrap_or_else(|error| {
        log::warn!(
            "[receiver-target] {} is unreadable ({error}); every receiver falls back to the General Agent",
            path.display()
        );
        Bindings::new()
    })
}

fn persist(path: &Path, bindings: &Bindings) -> Result<(), String> {
    let json = serde_json::to_string_pretty(bindings).map_err(|error| error.to_string())?;
    let staged = path.with_extension("json.tmp");
    std::fs::write(&staged, json)
        .and_then(|()| std::fs::rename(&staged, path))
        .map_err(|error| {
            let _ = std::fs::remove_file(&staged);
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

fn persist_failed(detail: String) -> ReceiverTargetError {
    log::warn!("[receiver-target] {detail}");
    ReceiverTargetError::PersistFailed
}

impl ServiceContext {
    fn receiver_target_liveness(&self, tab_id: &str) -> TargetLiveness {
        let (pty_id, chat_backed) = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(tab_id) else {
                return TargetLiveness::Absent;
            };
            (
                tab.pty_id.clone(),
                matches!(tab.backing, TabBacking::ChatBackend { .. }),
            )
        };
        let live = if chat_backed {
            self.tab_chat_backing(tab_id).is_some()
        } else {
            pty_id.is_some_and(|pty_id| self.pty.lock().get_session(&pty_id).is_some())
        };
        if live {
            TargetLiveness::Live
        } else {
            TargetLiveness::NotLive
        }
    }

    pub fn live_receiver_target(&self, plugin_id: &str) -> Option<String> {
        live_target(self.receiver_targets.get(plugin_id), |tab_id| {
            self.receiver_target_liveness(tab_id)
        })
    }

    pub fn receiver_target(
        &self,
        plugin_id: &str,
    ) -> Result<ReceiverTargetView, ReceiverTargetError> {
        if !self.webhook_registry.contains(plugin_id) {
            return Err(ReceiverTargetError::ReceiverNotFound);
        }
        let tab_id = self.receiver_targets.get(plugin_id);
        let live = tab_id
            .as_deref()
            .is_some_and(|tab_id| self.receiver_target_liveness(tab_id) == TargetLiveness::Live);
        Ok(ReceiverTargetView {
            plugin_id: plugin_id.to_string(),
            tab_id,
            live,
        })
    }

    pub fn set_receiver_target(
        &self,
        plugin_id: &str,
        tab_id: &str,
    ) -> Result<ReceiverTargetView, ReceiverTargetError> {
        if !self.webhook_registry.contains(plugin_id) {
            return Err(ReceiverTargetError::ReceiverNotFound);
        }
        let tab_id = self
            .mux
            .read()
            .resolve_tab_id(tab_id)
            .ok_or(ReceiverTargetError::TabNotFound)?;
        if self
            .receiver_targets
            .set(plugin_id, &tab_id)
            .map_err(persist_failed)?
        {
            self.publish_receiver_targets_changed();
        }
        self.receiver_target(plugin_id)
    }

    /// Clearing only returns events to the General Agent, so it needs no live receiver.
    pub fn clear_receiver_target(
        &self,
        plugin_id: &str,
    ) -> Result<ReceiverTargetView, ReceiverTargetError> {
        if self
            .receiver_targets
            .clear(plugin_id)
            .map_err(persist_failed)?
        {
            self.publish_receiver_targets_changed();
        }
        Ok(ReceiverTargetView {
            plugin_id: plugin_id.to_string(),
            tab_id: None,
            live: false,
        })
    }

    /// The action comes from the manifest the host registered, never from the caller.
    pub fn run_tab_menu_item(
        &self,
        plugin_id: &str,
        item_id: &str,
        tab_id: &str,
    ) -> Result<TabMenuState, ReceiverTargetError> {
        let items = self
            .tab_menu_registry
            .get(plugin_id)
            .ok_or(ReceiverTargetError::MenuItemNotFound)?;
        let item = items
            .iter()
            .find(|item| item.id == item_id)
            .ok_or(ReceiverTargetError::MenuItemNotFound)?;
        match item.action {
            TabMenuAction::ReceiverTarget => {
                let tab_id = self
                    .mux
                    .read()
                    .resolve_tab_id(tab_id)
                    .ok_or(ReceiverTargetError::TabNotFound)?;
                if self.receiver_targets.get(plugin_id).as_deref() == Some(tab_id.as_str()) {
                    self.clear_receiver_target(plugin_id)?;
                } else {
                    self.set_receiver_target(plugin_id, &tab_id)?;
                }
            }
        }
        Ok(self.tab_menu_state())
    }

    pub fn tab_menu_state(&self) -> TabMenuState {
        let mut plugin_ids = self.tab_menu_registry.ids();
        plugin_ids.sort();
        let items = plugin_ids
            .into_iter()
            .filter_map(|plugin_id| {
                let items = self.tab_menu_registry.get(&plugin_id)?;
                Some((plugin_id, items))
            })
            .flat_map(|(plugin_id, items)| {
                items
                    .iter()
                    .map(|item| TabMenuEntry {
                        plugin_id: plugin_id.clone(),
                        item_id: item.id.clone(),
                        label: item.label.clone(),
                        action: item.action,
                        icon_svg: item.icon_svg.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        TabMenuState {
            items,
            receiver_targets: self.receiver_targets.bindings(),
        }
    }

    pub(crate) fn forget_receiver_targets_for_tab(&self, tab_id: &str) {
        match self.receiver_targets.forget_tab(tab_id) {
            Ok(true) => self.publish_receiver_targets_changed(),
            Ok(false) => {}
            Err(detail) => log::warn!("[receiver-target] {detail}"),
        }
    }

    fn publish_receiver_targets_changed(&self) {
        self.events
            .publish(crate::api::events::SystemEvent::ContributionsChanged {
                kind: RECEIVER_TARGET_CONTRIBUTION_KIND.to_string(),
            });
    }
}

#[cfg(test)]
mod parity_tests;
#[cfg(test)]
mod tests;
