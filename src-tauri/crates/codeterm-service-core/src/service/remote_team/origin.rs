use super::{member_key, OutgoingTransfer, RemoteTeamMember, RemoteWorkerInfo, RemoteWorkerLookup};
use crate::mesh::transport::PeerClient;
use crate::multiplexer::{state::MultiplexerState, types::Tab};
use crate::service::remote_worker_transfer::{RemoteWorkerTransfer, RemoteWorkerTransferReceipt};
use crate::service::{AuthenticatedPlanCaller, MuxEmitCause, ServiceContext};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[async_trait::async_trait]
pub trait RemoteTeamPeer: Send + Sync {
    async fn post(&self, path: &str, body: Value) -> Result<Value, String>;
}

#[async_trait::async_trait]
impl RemoteTeamPeer for PeerClient {
    async fn post(&self, path: &str, body: Value) -> Result<Value, String> {
        self.auth_post_json(path, &body).await
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RemoteTransferTarget {
    pub orchestrator_tab_id: String,
    pub team_id: String,
}

pub(super) fn validate_actor(
    mux: &MultiplexerState,
    caller: &AuthenticatedPlanCaller,
) -> Result<Option<String>, String> {
    if caller.tab_id.is_none() && caller.session_id.is_none() && caller.binding_nonce.is_none() {
        return Ok(None);
    }
    let tab = caller
        .tab_id
        .as_deref()
        .and_then(|id| mux.get_tab(id))
        .ok_or("remote transfer caller tab is missing")?;
    if !tab.launch_session_authenticated()
        || caller
            .session_id
            .as_deref()
            .is_some_and(|session| tab.agent_session_id.as_deref() != Some(session))
        || caller.binding_nonce.as_deref() != tab.session_binding_nonce.as_deref()
    {
        return Err("remote transfer caller session is foreign or stale".into());
    }
    Ok(tab.agent_session_id.clone())
}

pub(super) fn target_for_tab(
    mux: &MultiplexerState,
    destination: &str,
) -> Result<RemoteTransferTarget, String> {
    let tab = mux
        .get_tab(destination)
        .ok_or("destination Orchestrator not found")?;
    if !tab.anchors_team()
        || !tab.has_team_orchestrator_authority()
        || !tab.launch_session_authenticated()
        || tab.report_target_mesh_peer.is_some()
        || !crate::service::plan_reconcile::tab_has_live_agent(tab)
    {
        return Err("destination must be a live local Team Orchestrator".into());
    }
    let team_id = tab
        .team_id
        .clone()
        .ok_or("destination Orchestrator has no Team")?;
    let team = mux.get_team(&team_id).ok_or("destination Team not found")?;
    if team.lifecycle == codeterm_types::team::TeamLifecycle::Closing {
        return Err("destination Team is closing".into());
    }
    let anchors = mux
        .team_member_tab_ids(&team_id)
        .into_iter()
        .filter(|id| mux.get_tab(id).is_some_and(Tab::anchors_team))
        .count();
    if anchors != 1 {
        return Err("destination Team has ambiguous Orchestrator anchors".into());
    }
    Ok(RemoteTransferTarget {
        orchestrator_tab_id: destination.into(),
        team_id,
    })
}

impl ServiceContext {
    pub fn remote_team_target(&self, team: &str) -> Result<RemoteTransferTarget, String> {
        let mux = self.mux.read();
        let anchor = mux
            .team_member_tab_ids(team)
            .into_iter()
            .find(|id| mux.get_tab(id).is_some_and(Tab::anchors_team))
            .ok_or("Team has no live local Orchestrator")?;
        target_for_tab(&mux, &anchor)
    }

    pub fn remote_team_peer(&self, name: &str) -> Result<(String, PeerClient), String> {
        let handle = self
            .try_ext::<crate::MeshStateHandle>()
            .ok_or("Mesh is unavailable")?;
        let mesh = handle.lock();
        let peer = crate::mesh::paired_resolve::resolve_paired_by_name(&mesh.paired_peers, name)
            .map_err(|error| format!("paired peer {name} cannot be resolved: {error:?}"))?;
        if peer.access_level != crate::mesh::types::AccessLevel::Full {
            return Err("remote Team operations require Full mesh access".into());
        }
        if !peer.online {
            return Err(format!(
                "remote Team peer {name} is offline; membership is retained"
            ));
        }
        let client = mesh
            .client_for(&peer.instance_id)
            .ok_or("paired peer is unavailable or its admission is closed")?;
        Ok((peer.instance_id.clone(), client))
    }

    pub async fn reparent_remote_worker(
        &self,
        caller: AuthenticatedPlanCaller,
        peer: &str,
        worker: &str,
        destination: &str,
    ) -> Result<RemoteTeamMember, String> {
        let (peer_id, client) = self.remote_team_peer(peer)?;
        self.reparent_remote_worker_with_peer(caller, &peer_id, worker, destination, &client)
            .await
    }

    pub async fn reparent_remote_worker_with_peer(
        &self,
        caller: AuthenticatedPlanCaller,
        peer_id: &str,
        worker: &str,
        destination: &str,
        peer: &impl RemoteTeamPeer,
    ) -> Result<RemoteTeamMember, String> {
        let (actor_session, target) = {
            let mux = self.mux.read();
            (
                validate_actor(&mux, &caller)?,
                target_for_tab(&mux, destination)?,
            )
        };
        let store = self.remote_teams.clone();
        let peer_id = peer_id.to_string();
        let worker = worker.to_string();
        let key = member_key(&peer_id, &worker);
        let data = super::super::blocking::run(move || store.snapshot()).await?;
        if data.closing_teams.contains(&target.team_id)
            || data
                .members
                .get(&key)
                .is_some_and(|member| data.closing_teams.contains(&member.team_id))
        {
            return Err("remote Team membership is frozen for close".into());
        }
        let same_actor = |op: &&OutgoingTransfer| {
            op.actor_tab_id == caller.tab_id && op.actor_session_id == actor_session
        };
        let pending = data
            .outgoing
            .values()
            .filter(|op| {
                op.peer_id == peer_id && op.request.worker_tab_id == worker && !op.completed
            })
            .collect::<Vec<_>>();
        let operation = if let Some(operation) = pending.first() {
            if operation.member.orchestrator_tab_id != destination
                || operation.member.team_id != target.team_id
                || !same_actor(operation)
            {
                return Err("remote Worker has an unresolved transfer; its authorized caller must recover it".into());
            }
            (*operation).clone()
        } else if let Some(operation) = data
            .outgoing
            .values()
            .filter(|op| {
                op.peer_id == peer_id
                    && op.request.worker_tab_id == worker
                    && op.member.orchestrator_tab_id == destination
                    && op.completed
            })
            .find(same_actor)
        {
            if data.members.get(&key) == Some(&operation.member)
                && operation.member.team_id == target.team_id
            {
                return Ok(operation.member.clone());
            }
            return Err("completed transfer receipt was superseded; use the current parent".into());
        } else {
            let lookup = RemoteWorkerLookup {
                worker_tab_id: worker.clone(),
                caller_tab_id: caller.tab_id.clone(),
                owner_confirmed: caller.tab_id.is_none(),
            };
            let info: RemoteWorkerInfo = serde_json::from_value(
                peer.post("/api/mesh/worker-transfer-info", json!(lookup))
                    .await?,
            )
            .map_err(|error| error.to_string())?;
            if info.tab_id != worker || info.session_id.is_empty() {
                return Err("peer returned a different Worker identity".into());
            }
            if caller
                .tab_id
                .as_deref()
                .is_some_and(|caller| caller != info.parent_tab_id)
            {
                return Err("only the current parent or owner may transfer this Worker".into());
            }
            let operation_id = uuid::Uuid::new_v4().to_string();
            let member = RemoteTeamMember {
                peer_id: peer_id.clone(),
                tab_id: worker.clone(),
                session_id: info.session_id.clone(),
                title: info.title,
                provider: info.provider,
                orchestrator_tab_id: destination.into(),
                team_id: target.team_id.clone(),
                operation_id: operation_id.clone(),
                finalized: false,
            };
            let operation = OutgoingTransfer {
                operation_id: operation_id.clone(),
                peer_id: peer_id.clone(),
                actor_tab_id: caller.tab_id.clone(),
                actor_session_id: actor_session,
                request: RemoteWorkerTransfer {
                    operation_id,
                    worker_tab_id: worker.clone(),
                    worker_session_id: info.session_id,
                    expected_parent_tab_id: info.parent_tab_id,
                    destination_tab_id: destination.into(),
                    destination_team_id: Some(target.team_id.clone()),
                    caller_tab_id: caller.tab_id.clone(),
                    owner_confirmed: caller.tab_id.is_none(),
                },
                member,
                completed: false,
            };
            let service = self.clone();
            let staged = operation.clone();
            let caller = caller.clone();
            let destination = destination.to_string();
            super::super::blocking::run(move || {
                let mux = service.mux.read();
                validate_actor(&mux, &caller)?;
                if target_for_tab(&mux, &destination)? != target {
                    return Err("destination changed during transfer preparation".into());
                }
                service.remote_teams.update(|data| {
                    if data.closing_teams.contains(&staged.member.team_id)
                        || data
                            .members
                            .get(&key)
                            .is_some_and(|member| data.closing_teams.contains(&member.team_id))
                    {
                        return Err("remote Team is closing".into());
                    }
                    if data.outgoing.values().any(|op| {
                        !op.completed
                            && op.peer_id == staged.peer_id
                            && op.request.worker_tab_id == staged.request.worker_tab_id
                    }) {
                        return Err("another remote transfer is already pending".into());
                    }
                    data.outgoing.insert(staged.operation_id.clone(), staged);
                    Ok(())
                })
            })
            .await?;
            operation
        };
        let receipt: RemoteWorkerTransferReceipt = serde_json::from_value(
            peer.post("/api/mesh/worker-transfer", json!(operation.request))
                .await?,
        )
        .map_err(|error| error.to_string())?;
        if receipt.operation_id != operation.operation_id
            || receipt.worker_tab_id != worker
            || receipt.worker_session_id != operation.member.session_id
            || receipt.origin_peer_id.is_empty()
            || receipt.destination_tab_id != destination
            || receipt.previous_parent_tab_id != operation.request.expected_parent_tab_id
        {
            return Err(
                "peer returned a mismatched transfer receipt; recover the pending operation".into(),
            );
        }
        let store = self.remote_teams.clone();
        let completed = operation.clone();
        super::super::blocking::run(move || {
            store.update(|data| {
                let pending = data
                    .outgoing
                    .get_mut(&completed.operation_id)
                    .ok_or("transfer intent disappeared")?;
                if pending.request != completed.request {
                    return Err("transfer intent changed".into());
                }
                pending.completed = true;
                data.members.insert(
                    member_key(&completed.peer_id, &completed.member.tab_id),
                    completed.member,
                );
                Ok(())
            })
        })
        .await?;
        let registry = self.remote_workers.clone();
        super::super::blocking::run(move || {
            registry.forget_by_remote_tab(&worker, Some(&peer_id), None);
            Ok(())
        })
        .await?;
        self.emit_remote_workers_changed(destination);
        self.emit_state_changed(MuxEmitCause::Team);
        Ok(operation.member)
    }
}
