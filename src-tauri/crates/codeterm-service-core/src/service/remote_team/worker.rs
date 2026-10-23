use super::{member_key, IncomingTransfer};
use crate::multiplexer::types::Tab;
use crate::service::remote_worker_transfer::{
    apply_remote_worker_transfer, authorize_remote_worker_transfer, RemoteWorkerTransfer,
    RemoteWorkerTransferReceipt,
};
use crate::service::{MuxEmitCause, ServiceContext};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteWorkerLookup {
    pub worker_tab_id: String,
    pub caller_tab_id: Option<String>,
    pub owner_confirmed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteWorkerInfo {
    pub tab_id: String,
    pub session_id: String,
    pub parent_tab_id: String,
    pub title: String,
    pub provider: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteWorkerFinalize {
    pub worker_tab_id: String,
    pub worker_session_id: String,
    pub expected_parent_tab_id: String,
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteFinalizationReceipt {
    pub worker_tab_id: String,
    pub worker_session_id: String,
    pub outcome: String,
}

impl ServiceContext {
    fn normalized_remote_worker(&self, tab: &Tab, peer: &str) -> Result<Tab, String> {
        let mut normalized = tab.clone();
        if tab.report_target_mesh_peer.as_deref() != Some(peer) {
            let matched_owner = tab.ownership.as_ref().is_some_and(|owner| {
                owner.peer_id == peer
                    && tab.report_target_mesh_peer.as_deref() == Some(owner.peer_name.as_str())
            });
            let matched_pair = self
                .try_ext::<crate::MeshStateHandle>()
                .is_some_and(|mesh| {
                    mesh.lock().paired_peers.get(peer).is_some_and(|paired| {
                        tab.report_target_mesh_peer.as_deref()
                            == Some(paired.instance_name.as_str())
                    })
                });
            if !matched_owner && !matched_pair {
                return Err("authenticated peer does not own the Worker report route".into());
            }
            normalized.report_target_mesh_peer = Some(peer.into());
        }
        Ok(normalized)
    }

    pub fn inspect_remote_worker(
        &self,
        peer: &str,
        lookup: &RemoteWorkerLookup,
    ) -> Result<RemoteWorkerInfo, String> {
        let mux = self.mux.read();
        let tab = mux
            .get_tab(&lookup.worker_tab_id)
            .ok_or("remote Worker tab not found")?;
        let session_id = tab
            .agent_session_id
            .clone()
            .ok_or("remote Worker has no session")?;
        let parent = tab
            .report_target_tab_id
            .clone()
            .ok_or("remote Worker has no report parent")?;
        let request = RemoteWorkerTransfer {
            operation_id: "inspection".into(),
            worker_tab_id: tab.id.clone(),
            worker_session_id: session_id.clone(),
            expected_parent_tab_id: parent.clone(),
            destination_tab_id: parent.clone(),
            destination_team_id: None,
            caller_tab_id: lookup.caller_tab_id.clone(),
            owner_confirmed: lookup.owner_confirmed,
        };
        let normalized = self.normalized_remote_worker(tab, peer)?;
        authorize_remote_worker_transfer(&normalized, peer, &request)?;
        Ok(RemoteWorkerInfo {
            tab_id: tab.id.clone(),
            session_id,
            parent_tab_id: parent,
            title: tab.title.clone(),
            provider: tab
                .agent_type
                .clone()
                .ok_or("remote Worker has no provider")?,
        })
    }

    pub(crate) fn commit_remote_worker_transfer(
        &self,
        tab: &mut Tab,
        peer: &str,
        request: &RemoteWorkerTransfer,
    ) -> Result<RemoteWorkerTransferReceipt, String> {
        let key = member_key(peer, &request.worker_tab_id);
        let data = self.remote_teams.snapshot()?;
        if let Some(existing) = data.incoming.get(&request.operation_id) {
            if existing.origin_peer_id != peer || existing.request != *request {
                return Err(
                    "remote transfer operation id conflicts with its durable receipt".into(),
                );
            }
            if data.latest_incoming.get(&key) == Some(&request.operation_id) {
                let mut normalized = self.normalized_remote_worker(tab, peer)?;
                replay_committed(&mut normalized, peer, request)?;
                tab.report_target_tab_id = normalized.report_target_tab_id;
                tab.report_target_mesh_peer = normalized.report_target_mesh_peer;
                tab.ownership = normalized.ownership;
            }
            return Ok(existing.receipt.clone());
        }
        let mut next = self.normalized_remote_worker(tab, peer)?;
        let receipt = apply_remote_worker_transfer(&mut next, peer, request)?;
        self.remote_teams.update(|data| {
            data.incoming.insert(
                request.operation_id.clone(),
                IncomingTransfer {
                    origin_peer_id: peer.into(),
                    request: request.clone(),
                    receipt: receipt.clone(),
                },
            );
            data.latest_incoming
                .insert(key, request.operation_id.clone());
            Ok(())
        })?;
        tab.report_target_tab_id = next.report_target_tab_id;
        tab.report_target_mesh_peer = next.report_target_mesh_peer;
        tab.ownership = next.ownership;
        Ok(receipt)
    }

    pub fn restore_remote_worker_transfers(&self) {
        let Ok(data) = self.remote_teams.snapshot() else {
            return;
        };
        let mut mux = self.mux.write();
        for operation in data.latest_incoming.values() {
            let Some(transfer) = data.incoming.get(operation) else {
                continue;
            };
            if let Some(tab) = mux.get_tab_mut(&transfer.request.worker_tab_id) {
                let result = self
                    .normalized_remote_worker(tab, &transfer.origin_peer_id)
                    .and_then(|mut normalized| {
                        // The receipt authorizes route recovery without authenticating the restored session.
                        normalized.reported_session_binding_nonce =
                            normalized.session_binding_nonce.clone();
                        replay_committed(
                            &mut normalized,
                            &transfer.origin_peer_id,
                            &transfer.request,
                        )?;
                        tab.report_target_tab_id = normalized.report_target_tab_id;
                        tab.report_target_mesh_peer = normalized.report_target_mesh_peer;
                        tab.ownership = normalized.ownership;
                        Ok(())
                    });
                if let Err(error) = result {
                    log::debug!("remote transfer recovery retained current binding: {error}");
                }
            }
        }
    }

    pub fn finalize_remote_worker(
        &self,
        peer: &str,
        request: &RemoteWorkerFinalize,
    ) -> Result<RemoteFinalizationReceipt, String> {
        let decision = {
            let mux = self.mux.read();
            match mux.get_tab(&request.worker_tab_id) {
                None => "absent",
                Some(tab)
                    if tab.agent_session_id.as_deref() != Some(&request.worker_session_id)
                        || tab.report_target_mesh_peer.as_deref() != Some(peer)
                        || tab.report_target_tab_id.as_deref()
                            != Some(&request.expected_parent_tab_id) =>
                {
                    "released_stale_binding"
                }
                Some(tab) => {
                    let check = RemoteWorkerTransfer {
                        operation_id: "finalize".into(),
                        worker_tab_id: request.worker_tab_id.clone(),
                        worker_session_id: request.worker_session_id.clone(),
                        expected_parent_tab_id: request.expected_parent_tab_id.clone(),
                        destination_tab_id: request.expected_parent_tab_id.clone(),
                        destination_team_id: None,
                        caller_tab_id: Some(request.expected_parent_tab_id.clone()),
                        owner_confirmed: false,
                    };
                    authorize_remote_worker_transfer(tab, peer, &check)?;
                    if !mux.get_children_ids(&tab.id).is_empty() {
                        return Err("remote Worker has child tabs".into());
                    }
                    if !request.force
                        && tab.agent_state.as_ref().is_some_and(|state| {
                            matches!(
                                state.state,
                                crate::agent::offline_state_classifier::TimelineState::Working
                            )
                        })
                    {
                        return Err("remote Worker is working; Team close requires force".into());
                    }
                    "closed"
                }
            }
        };
        if decision == "closed" {
            self.close_tab_with_precondition(
                &request.worker_tab_id,
                crate::multiplexer::types::TabCloseReason::User,
                |mux, id| {
                    let tab = mux
                        .get_tab(id)
                        .ok_or("remote Worker disappeared before finalization")?;
                    if tab.agent_session_id.as_deref() != Some(&request.worker_session_id)
                        || tab.report_target_mesh_peer.as_deref() != Some(peer)
                        || tab.report_target_tab_id.as_deref()
                            != Some(&request.expected_parent_tab_id)
                        || tab.task_binding.is_some()
                        || tab.team_id.is_some()
                        || tab.parent_tab_id.is_some()
                        || !mux.get_children_ids(id).is_empty()
                        || !tab.launch_session_authenticated()
                    {
                        return Err("remote Worker binding changed before finalization".into());
                    }
                    if !request.force
                        && tab.agent_state.as_ref().is_some_and(|state| {
                            matches!(
                                state.state,
                                crate::agent::offline_state_classifier::TimelineState::Working
                            )
                        })
                    {
                        return Err("remote Worker is working; Team close requires force".into());
                    }
                    Ok(())
                },
            )?;
            self.emit_state_changed(MuxEmitCause::Team);
        }
        Ok(RemoteFinalizationReceipt {
            worker_tab_id: request.worker_tab_id.clone(),
            worker_session_id: request.worker_session_id.clone(),
            outcome: decision.into(),
        })
    }
}

fn replay_committed(
    tab: &mut Tab,
    peer: &str,
    request: &RemoteWorkerTransfer,
) -> Result<(), String> {
    let mut check = request.clone();
    if tab.report_target_tab_id.as_deref() == Some(&request.destination_tab_id) {
        check.expected_parent_tab_id = request.destination_tab_id.clone();
        check.caller_tab_id = Some(request.destination_tab_id.clone());
    }
    apply_remote_worker_transfer(tab, peer, &check)?;
    Ok(())
}
