use super::origin::{target_for_tab, validate_actor};
use crate::multiplexer::types::AgentRole;
use crate::service::{AuthenticatedPlanCaller, MuxEmitCause, ServiceContext};
use serde_json::{json, Value};

impl ServiceContext {
    pub async fn adopt_remote_team_member(
        &self,
        caller: AuthenticatedPlanCaller,
        team: &str,
        worker: &str,
        peer: &str,
    ) -> Result<Value, String> {
        let destination = self.remote_team_target(team)?;
        self.reparent_worker(
            caller,
            worker.to_string(),
            destination.orchestrator_tab_id,
            Some(peer.to_string()),
        )
        .await
    }

    pub async fn reparent_worker(
        &self,
        caller: AuthenticatedPlanCaller,
        worker: String,
        destination: String,
        peer: Option<String>,
    ) -> Result<Value, String> {
        if let Some(peer) = peer {
            let member = self
                .reparent_remote_worker(caller, &peer, &worker, &destination)
                .await?;
            return Ok(
                json!({ "team_id": member.team_id, "tab_id": member.tab_id, "peer": member.peer_id, "operation_id": member.operation_id, "member": member }),
            );
        }
        let service = self.clone();
        crate::service::blocking::run(move || {
            service.reparent_local_worker(&caller, &worker, &destination)
        })
        .await
    }

    pub fn reparent_local_worker(
        &self,
        caller: &AuthenticatedPlanCaller,
        worker: &str,
        destination: &str,
    ) -> Result<Value, String> {
        let (team_id, session_id) = {
            let mut mux = self.mux.write();
            validate_actor(&mux, caller)?;
            let target = target_for_tab(&mux, destination)?;
            let data = self.remote_teams.snapshot()?;
            if data.closing_teams.contains(&target.team_id) {
                return Err("destination Team is closing".into());
            }
            let tab = mux.get_tab(worker).ok_or("Worker tab not found")?;
            if tab.role == Some(AgentRole::Manager)
                || !tab.launch_session_authenticated()
                || tab.task_binding.is_some()
                || tab.report_target_mesh_peer.is_some()
                || tab.ownership.is_some()
                || !mux.get_children_ids(worker).is_empty()
                || mux.find_workspace_for_tab(worker) != mux.find_workspace_for_tab(destination)
            {
                return Err("local reparent requires an unbound authenticated leaf Worker in the destination workspace".into());
            }
            if tab
                .team_id
                .as_ref()
                .is_some_and(|team| data.closing_teams.contains(team))
            {
                return Err("source Team is closing".into());
            }
            let parent = tab
                .parent_tab_id
                .as_deref()
                .or(tab.report_target_tab_id.as_deref())
                .or(tab.spawned_by_tab_id.as_deref());
            if caller
                .tab_id
                .as_deref()
                .is_some_and(|caller| Some(caller) != parent)
            {
                return Err("only the current parent or owner may reparent this Worker".into());
            }
            let session = tab.agent_session_id.clone();
            let depth = mux
                .get_tab(destination)
                .ok_or("destination disappeared")?
                .depth
                .saturating_add(1);
            let tab = mux.get_tab_mut(worker).ok_or("Worker disappeared")?;
            tab.parent_tab_id = Some(destination.into());
            tab.team_id = Some(target.team_id.clone());
            tab.report_target_tab_id = Some(destination.into());
            tab.depth = depth;
            (target.team_id, session)
        };
        crate::service::session_persist::persist_session_after_mutation(
            self,
            crate::service::session_persist::SessionPersistUrgency::Prompt,
        )?;
        self.emit_state_changed(MuxEmitCause::Team);
        Ok(
            json!({ "team_id": team_id, "tab_id": worker, "destination_tab_id": destination, "session_id": session_id }),
        )
    }
}
