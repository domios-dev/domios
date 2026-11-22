use crate::agent::chat_types::ChatMsg;
use crate::multiplexer::types::{ContextEpochArchive, Tab, TabBacking};
use crate::service::live_transcript::{
    BindingTarget, ConciergeEpoch, EpochHistory, TranscriptBinding, TranscriptOwnerState,
    TranscriptStore,
};
use crate::service::{tab_chat::session_segments, ServiceContext};

pub(in crate::service) fn is_current_epoch(tab: &Tab, epoch: &ConciergeEpoch) -> bool {
    tab.id == epoch.runtime_tab_id
        && tab.hidden
        && !tab.show_in_tabbar
        && tab.launch_session_authenticated()
        && tab.agent_type.as_deref() == Some(epoch.provider.as_str())
        && tab.agent_session_id.as_deref() == Some(epoch.session_id.as_str())
        && tab.session_binding_nonce.as_deref() == Some(epoch.binding_nonce.as_str())
        && matches!(&tab.backing, TabBacking::ChatBackend { plugin_id, session_id }
            if plugin_id == &epoch.provider && session_id == &epoch.session_id)
}

fn require_live_owner(store: &TranscriptStore, owner_tab_id: &str) -> Result<(), String> {
    match store.owner_state() {
        TranscriptOwnerState::Open { live_tab_id } if live_tab_id == owner_tab_id => Ok(()),
        _ => Err("Transcript owner is not live".into()),
    }
}

fn active_epoch(
    store: &TranscriptStore,
    owner_tab_id: &str,
    binding: &TranscriptBinding,
) -> Result<ConciergeEpoch, String> {
    require_live_owner(store, owner_tab_id)?;
    store.cursor(binding).map_err(|error| error.to_string())?;
    let BindingTarget::Attached { epoch_id } = &binding.target else {
        return Err("Transcript has no attached router".into());
    };
    match store.epoch(epoch_id).map_err(|error| error.to_string())? {
        Some(EpochHistory {
            epoch,
            archive: None,
        }) => Ok(epoch),
        _ => Err("Transcript router epoch is unavailable".into()),
    }
}

impl ServiceContext {
    /// Pass the live owner ID from the authenticated transcript request.
    pub fn attached_transcript_history(
        &self,
        store: &TranscriptStore,
        authorized_owner_tab_id: &str,
        epoch_id: &str,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<ChatMsg>, Option<usize>), String> {
        if !(1..=100).contains(&limit) {
            return Err("Invalid transcript epoch page size".into());
        }
        let reader = store.pin_reader().map_err(|error| error.to_string())?;
        let store = reader.store();
        require_live_owner(store, authorized_owner_tab_id)?;
        if self.mux.read().get_tab(authorized_owner_tab_id).is_none() {
            return Err("Transcript owner tab is absent".into());
        }
        let history = store
            .epoch(epoch_id)
            .map_err(|error| error.to_string())?
            .ok_or("Transcript epoch is absent")?;
        let rows = if let Some(archive) = history.archive {
            let archive: ContextEpochArchive =
                serde_json::from_str(&archive.history_ref).map_err(|error| error.to_string())?;
            if archive.session_id != history.epoch.session_id
                || archive.provider != history.epoch.provider
            {
                return Err("Transcript archive identity mismatch".into());
            }
            session_segments::retained_chat_backend_epoch(self, &archive)?
        } else {
            let binding = store.binding();
            let current = active_epoch(store, authorized_owner_tab_id, &binding)?;
            if current != history.epoch {
                return Err("Transcript epoch has no retained archive".into());
            }
            let rows = self.attached_transcript_epoch_rows(&current)?;
            if active_epoch(store, authorized_owner_tab_id, &binding)? != current {
                return Err("Transcript epoch changed during history read".into());
            }
            rows
        };
        require_live_owner(store, authorized_owner_tab_id)?;
        if self.mux.read().get_tab(authorized_owner_tab_id).is_none() {
            return Err("Transcript owner tab closed during history read".into());
        }
        Ok(thinking_and_tools_page(rows, offset, limit))
    }
}

fn thinking_and_tools_page(
    rows: Vec<ChatMsg>,
    offset: usize,
    limit: usize,
) -> (Vec<ChatMsg>, Option<usize>) {
    let mut result = rows
        .into_iter()
        .filter(|row| {
            matches!(
                row.kind.as_str(),
                "thinking" | "tool" | "tool_call" | "tool_result"
            )
        })
        .skip(offset)
        .take(limit + 1)
        .collect::<Vec<_>>();
    let next = if result.len() > limit {
        result.pop();
        offset.checked_add(limit)
    } else {
        None
    };
    (result, next)
}

#[cfg(test)]
mod tests;
