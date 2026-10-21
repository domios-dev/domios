use crate::multiplexer::types::TabBacking;
use crate::plugin::tab_menu::TabMenuAction;
use crate::service::ServiceContext;
pub(crate) use codeterm_surface::receiver_target::LocalRecordingTargets;
pub use codeterm_surface::receiver_target::{
    host_json, live_target, ReceiverTargetError, ReceiverTargetStore, ReceiverTargetView,
    ReceiverTargets, TabMenuEntry, TabMenuState, TargetLiveness, RECEIVER_TARGET_CONTRIBUTION_KIND,
};

fn persist_failed(detail: String) -> ReceiverTargetError {
    log::warn!("[receiver-target] {detail}");
    ReceiverTargetError::PersistFailed
}

impl ServiceContext {
    pub(crate) fn receiver_target_eligible(&self, plugin_id: &str) -> bool {
        self.ext::<codeterm_plugin::WebhookRegistryHandle>()
            .contains(plugin_id)
            || self
                .ext::<codeterm_plugin::local_recording::LocalRecordingRegistryHandle>()
                .contains(plugin_id)
    }

    fn receiver_target_store(&self, plugin_id: &str) -> &ReceiverTargetStore {
        if self
            .ext::<codeterm_plugin::local_recording::LocalRecordingRegistryHandle>()
            .contains(plugin_id)
        {
            &self
                .ext::<crate::service::receiver_target::LocalRecordingTargets>()
                .0
        } else {
            &self
                .ext::<crate::service::receiver_target::ReceiverTargets>()
                .0
        }
    }
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
        live_target(
            self.ext::<crate::service::receiver_target::ReceiverTargets>()
                .0
                .get(plugin_id),
            |tab_id| self.receiver_target_liveness(tab_id),
        )
    }

    pub fn receiver_target(
        &self,
        plugin_id: &str,
    ) -> Result<ReceiverTargetView, ReceiverTargetError> {
        if !self.receiver_target_eligible(plugin_id) {
            return Err(ReceiverTargetError::ReceiverNotFound);
        }
        let tab_id = self.receiver_target_store(plugin_id).get(plugin_id);
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
        if !self.receiver_target_eligible(plugin_id) {
            return Err(ReceiverTargetError::ReceiverNotFound);
        }
        let tab_id = self
            .mux
            .read()
            .resolve_tab_id(tab_id)
            .ok_or(ReceiverTargetError::TabNotFound)?;
        if self
            .receiver_target_store(plugin_id)
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
            .receiver_target_store(plugin_id)
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
            .ext::<codeterm_plugin::tab_menu::TabMenuRegistryHandle>()
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
                if self
                    .receiver_target_store(plugin_id)
                    .get(plugin_id)
                    .as_deref()
                    == Some(tab_id.as_str())
                {
                    self.clear_receiver_target(plugin_id)?;
                } else {
                    self.set_receiver_target(plugin_id, &tab_id)?;
                }
            }
        }
        Ok(self.tab_menu_state())
    }

    pub fn tab_menu_state(&self) -> TabMenuState {
        let mut plugin_ids = self
            .ext::<codeterm_plugin::tab_menu::TabMenuRegistryHandle>()
            .ids();
        plugin_ids.sort();
        let items = plugin_ids
            .into_iter()
            .filter_map(|plugin_id| {
                let items = self
                    .ext::<codeterm_plugin::tab_menu::TabMenuRegistryHandle>()
                    .get(&plugin_id)?;
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
                        icon_color: item.icon_color.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .collect();
        let mut receiver_targets = self
            .ext::<crate::service::receiver_target::ReceiverTargets>()
            .0
            .bindings();
        receiver_targets.extend(
            self.ext::<crate::service::receiver_target::LocalRecordingTargets>()
                .0
                .bindings(),
        );
        TabMenuState {
            items,
            receiver_targets,
        }
    }

    pub(crate) fn forget_receiver_targets_for_tab(&self, tab_id: &str) {
        self.local_recordings.close_tab(tab_id);
        match self
            .ext::<crate::service::receiver_target::ReceiverTargets>()
            .0
            .forget_tab(tab_id)
        {
            Ok(true) => self.publish_receiver_targets_changed(),
            Ok(false) => {}
            Err(detail) => log::warn!("[receiver-target] {detail}"),
        }
    }

    fn publish_receiver_targets_changed(&self) {
        self.events
            .publish(codeterm_events::SystemEvent::ContributionsChanged {
                kind: RECEIVER_TARGET_CONTRIBUTION_KIND.to_string(),
            });
    }
}

#[cfg(test)]
mod parity_tests;
#[cfg(test)]
mod recording_tests;
#[cfg(test)]
mod tests;
