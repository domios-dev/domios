use crate::{multiplexer::state::MultiplexerState, service::ServiceContext};
use serde_json::{json, Value};

pub fn local_team_member_rows(mux: &MultiplexerState, team: &str) -> Vec<Value> {
    let mut members = mux.snapshot().tabs.values().filter(|tab| tab.team_id.as_deref() == Some(team)).map(|tab| json!({
        "tab_id":tab.id, "title":tab.title, "role":tab.role_kind_id().unwrap_or("roleless"),
        "role_type":match tab.role { Some(crate::multiplexer::types::AgentRole::Manager) => "Manager", Some(crate::multiplexer::types::AgentRole::Worker) => "Worker", None => "roleless" },
        "role_profile":tab.role_profile, "anchors_team":tab.anchors_team(),
    })).collect::<Vec<_>>();
    members.sort_by(|a, b| a["tab_id"].as_str().cmp(&b["tab_id"].as_str()));
    members
}

impl ServiceContext {
    pub fn team_member_inventory(&self, team: &str) -> Result<Value, String> {
        let mut members = {
            let mux = self.mux.read();
            if mux.get_team(team).is_none() {
                return Err(format!("Team {team} not found"));
            }
            local_team_member_rows(&mux, team)
        };
        let data = self.remote_teams.snapshot()?;
        members.extend(data.members.values().filter(|member| member.team_id == team).map(|member| json!({
            "tab_id":format!("mesh:{}/{}",member.peer_id,member.tab_id), "remote_tab_id":member.tab_id,
            "peer":member.peer_id,"session_id":member.session_id,"provider":member.provider,
            "title":member.title,"role":"worker","role_type":"Worker","role_profile":"worker",
            "anchors_team":false,"remote":true,"team_id":member.team_id,"parent_tab_id":member.orchestrator_tab_id,
            "finalized":member.finalized,"operation_id":member.operation_id,
        })));
        let pending = data.outgoing.values().filter(|operation| !operation.completed && (operation.member.team_id == team || data.members.get(&super::member_key(&operation.peer_id,&operation.member.tab_id)).is_some_and(|member|member.team_id==team))).map(|operation| json!({
            "operation_id":operation.operation_id,"peer":operation.peer_id,"tab_id":operation.member.tab_id,"destination_tab_id":operation.member.orchestrator_tab_id,"state":"pending",
        })).collect::<Vec<_>>();
        Ok(
            json!({"team_id":team,"members":members,"remote_transfers":pending,"remote_membership_closing":data.closing_teams.contains(team)}),
        )
    }
}
