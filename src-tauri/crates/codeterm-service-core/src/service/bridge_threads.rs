//! Daemon wiring for per-tab Telegram threads: live tab identity, caller
//! resolution and lifecycle release. `ThreadService` owns the bindings.

use std::path::PathBuf;

use codeterm_bridge::bridge::threads::{
    authorize_open, CloseOutcome, OpenedThread, SenderIdentity, ThreadCaller, ThreadError,
    ThreadMenuState, ThreadTabs,
};

use super::general_agent::GENERAL_ALIAS;
use super::ServiceContext;
pub use codeterm_notifications::bridge_threads::{sender_identity, TabIdentity};

pub fn registry_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(crate::paths::dir_name())
        .join("bridge")
        .join("telegram-threads.json")
}

fn machine_name(configured: &str) -> String {
    let configured = configured.trim();
    if !configured.is_empty() {
        return configured.to_string();
    }
    hostname::get()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default()
}

impl ServiceContext {
    pub(crate) fn bridge_sender_identity(&self, tab_id: &str) -> Option<SenderIdentity> {
        let machine = machine_name(&self.configuration().mesh.instance_name);
        let mux = self.mux.read();
        let tab = mux.get_tab(tab_id)?;
        let is_general = mux.resolve_tab_id(GENERAL_ALIAS).as_deref() == Some(tab_id);
        let workspace = mux
            .find_workspace_for_tab(tab_id)
            .and_then(|id| mux.get_workspace(&id).map(|w| w.name.clone()))
            .unwrap_or_default();
        let identity = TabIdentity {
            id: tab_id.to_string(),
            title: tab.title.clone(),
            role: tab.role_profile.clone(),
            workspace,
            is_general,
        };
        drop(mux);
        Some(sender_identity(identity, machine))
    }

    pub(crate) fn bridge_live_tab_titles(&self) -> Vec<(String, String)> {
        self.mux
            .read()
            .all_tabs()
            .map(|tab| (tab.id.clone(), tab.title.clone()))
            .collect()
    }

    pub(crate) fn bridge_is_live_tab(&self, tab_id: &str) -> bool {
        self.mux.read().get_tab(tab_id).is_some()
    }

    pub fn bridge_is_general_tab(&self, tab_id: &str) -> bool {
        self.mux.read().resolve_tab_id(GENERAL_ALIAS).as_deref() == Some(tab_id)
    }

    /// The owner's private chat with the bot is addressed by their Telegram id.
    pub(crate) fn bridge_owner_chat_id(&self) -> Option<i64> {
        self.configuration()
            .integrations
            .telegram_allowed_users
            .iter()
            .find_map(|id| id.trim().parse::<i64>().ok())
    }

    fn canonical_tab_id(&self, tab_id: &str) -> String {
        let trimmed = tab_id.trim();
        self.mux
            .read()
            .resolve_tab_id(trimmed)
            .unwrap_or_else(|| trimmed.to_string())
    }

    /// `caller_tab_id: None` is the user acting through the tab's own menu.
    pub async fn open_bridge_thread(
        &self,
        caller_tab_id: Option<&str>,
        target_tab_id: &str,
        chat_id: Option<i64>,
    ) -> Result<OpenedThread, ThreadError> {
        let target = self.canonical_tab_id(target_tab_id);
        let caller = caller_tab_id.map(|id| self.canonical_tab_id(id));
        let port = crate::bridge_host::ServiceBridgeTabPort(self.clone());
        let thread_caller = match caller.as_deref() {
            Some(tab_id) => ThreadCaller::Tab {
                tab_id,
                is_general: port.is_general_tab(tab_id),
            },
            None => ThreadCaller::Owner,
        };
        authorize_open(thread_caller, &target)?;
        let chat_id = chat_id
            .or_else(|| self.bridge_owner_chat_id())
            .ok_or(ThreadError::NoOwnerChat)?;
        self.ext::<std::sync::Arc<codeterm_bridge::bridge::threads::ThreadService>>()
            .open(&port, thread_caller, &target, chat_id)
            .await
    }

    pub async fn close_bridge_thread(&self, tab_id: &str) -> Vec<CloseOutcome> {
        let tab_id = self.canonical_tab_id(tab_id);
        self.ext::<std::sync::Arc<codeterm_bridge::bridge::threads::ThreadService>>()
            .close_tab(&tab_id)
            .await
    }

    pub async fn bridge_thread_menu_state(&self, tab_id: &str) -> ThreadMenuState {
        let tab_id = self.canonical_tab_id(tab_id);
        let mut state = self
            .ext::<std::sync::Arc<codeterm_bridge::bridge::threads::ThreadService>>()
            .menu_state(&tab_id, self.bridge_owner_chat_id())
            .await;
        state.bridge_active = state.bridge_active
            && self
                .ext::<std::sync::Arc<crate::bridge::supervisor::BridgeSupervisor>>()
                .is_active();
        state
    }

    /// Retire a closed tab's threads off the close path; an unbound tab spawns nothing.
    pub(crate) fn release_bridge_threads(&self, tab_id: &str) {
        if !self
            .ext::<std::sync::Arc<codeterm_bridge::bridge::threads::ThreadService>>()
            .holds_topics(tab_id)
        {
            return;
        }
        let threads = self
            .ext::<std::sync::Arc<codeterm_bridge::bridge::threads::ThreadService>>()
            .clone();
        let tab_id = tab_id.to_string();
        crate::async_runtime::spawn(async move {
            for outcome in threads.close_tab(&tab_id).await {
                log::info!("[bridge-threads] tab {tab_id} released: {outcome:?}");
            }
        });
    }
}

#[cfg(test)]
mod telegram_topic_tests;

#[cfg(test)]
mod telegram_notification_order_tests;
