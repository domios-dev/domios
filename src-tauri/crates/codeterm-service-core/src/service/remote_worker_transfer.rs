use serde::{Deserialize, Serialize};

use crate::multiplexer::types::{AgentRole, Tab};
use crate::service::{MuxEmitCause, ServiceContext};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RemoteWorkerTransfer {
    pub operation_id: String,
    pub worker_tab_id: String,
    pub worker_session_id: String,
    pub expected_parent_tab_id: String,
    pub destination_tab_id: String,
    pub destination_team_id: Option<String>,
    pub caller_tab_id: Option<String>,
    #[serde(default)]
    pub owner_confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteWorkerTransferReceipt {
    pub operation_id: String,
    pub worker_tab_id: String,
    pub worker_session_id: String,
    pub origin_peer_id: String,
    pub previous_parent_tab_id: String,
    pub destination_tab_id: String,
}

pub fn authorize_remote_worker_transfer(
    tab: &Tab,
    origin_peer_id: &str,
    request: &RemoteWorkerTransfer,
) -> Result<(), String> {
    if request.operation_id.trim().is_empty()
        || origin_peer_id.trim().is_empty()
        || request.destination_tab_id.trim().is_empty()
        || request.expected_parent_tab_id.trim().is_empty()
    {
        return Err("remote Worker transfer requires an origin and exact parent targets".into());
    }
    if tab.id != request.worker_tab_id
        || request.worker_session_id.trim().is_empty()
        || tab.agent_session_id.as_deref() != Some(request.worker_session_id.as_str())
        || !tab.launch_session_authenticated()
    {
        return Err(
            "remote Worker transfer refused: Worker session changed or is unverified".into(),
        );
    }
    let own_peer = tab.ownership.as_ref().is_some_and(|owner| {
        owner.peer_id == origin_peer_id
            && tab
                .report_target_mesh_peer
                .as_deref()
                .is_some_and(|peer| peer == origin_peer_id || peer == owner.peer_name)
    });
    if (tab.report_target_mesh_peer.as_deref() != Some(origin_peer_id) && !own_peer)
        || tab.parent_tab_id.is_some()
        || tab.team_id.is_some()
        || tab.role == Some(AgentRole::Manager)
        || tab
            .ownership
            .as_ref()
            .is_some_and(|owner| owner.peer_id != origin_peer_id)
    {
        return Err("remote Worker transfer refused: peer does not own this remote Worker".into());
    }
    if tab.task_binding.is_some() {
        return Err(
            "remote Worker transfer refused: task-bound Workers require a Plan ownership transfer"
                .into(),
        );
    }
    if tab.report_target_tab_id.as_deref() != Some(request.expected_parent_tab_id.as_str()) {
        return Err("remote Worker transfer refused: current parent changed".into());
    }
    if !request.owner_confirmed
        && request.caller_tab_id.as_deref() != Some(request.expected_parent_tab_id.as_str())
    {
        return Err(
            "remote Worker transfer refused: only the current parent or owner may transfer".into(),
        );
    }
    Ok(())
}

pub fn apply_remote_worker_transfer(
    tab: &mut Tab,
    origin_peer_id: &str,
    request: &RemoteWorkerTransfer,
) -> Result<RemoteWorkerTransferReceipt, String> {
    authorize_remote_worker_transfer(tab, origin_peer_id, request)?;
    tab.report_target_tab_id = Some(request.destination_tab_id.clone());
    tab.report_target_mesh_peer = Some(origin_peer_id.into());
    if let Some(owner) = tab.ownership.as_mut() {
        owner.controller_tab_id = Some(request.destination_tab_id.clone());
        owner.controller_title = request.destination_tab_id.clone();
        owner.team = request.destination_team_id.as_ref().map(|id| {
            codeterm_types::pty::RemoteOwnershipTeam {
                id: id.clone(),
                title: None,
            }
        });
    }
    Ok(RemoteWorkerTransferReceipt {
        operation_id: request.operation_id.clone(),
        worker_tab_id: tab.id.clone(),
        worker_session_id: request.worker_session_id.clone(),
        origin_peer_id: origin_peer_id.to_string(),
        previous_parent_tab_id: request.expected_parent_tab_id.clone(),
        destination_tab_id: request.destination_tab_id.clone(),
    })
}

impl ServiceContext {
    pub fn transfer_remote_worker_report_target(
        &self,
        origin_peer_id: &str,
        request: &RemoteWorkerTransfer,
    ) -> Result<RemoteWorkerTransferReceipt, String> {
        if request.operation_id.trim().is_empty() {
            return Err("remote Worker transfer requires an operation id".into());
        }
        let receipt = {
            let mut mux = self.mux.write();
            if !mux.get_children_ids(&request.worker_tab_id).is_empty() {
                return Err("remote Worker transfer refused: Worker has child tabs".into());
            }
            let tab = mux.get_tab_mut(&request.worker_tab_id).ok_or_else(|| {
                "remote Worker transfer refused: Worker tab not found".to_string()
            })?;
            self.commit_remote_worker_transfer(tab, origin_peer_id, request)?
        };
        self.schedule_session_persist_debounced();
        self.emit_state_changed(MuxEmitCause::Team);
        Ok(receipt)
    }
}

#[cfg(test)]
mod tests;
