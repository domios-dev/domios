use super::{tab_credentials::TabCaller, ServiceContext};

#[derive(Clone, Copy)]
pub enum TabMutationCaller<'a> {
    Host,
    Tab(&'a TabCaller),
}

#[derive(Clone, Copy, Default)]
pub struct TabMutationScope<'a> {
    pub parent_tab_id: Option<&'a str>,
    pub plan_id: Option<&'a str>,
    pub creates_team: bool,
    pub self_promotion: bool,
}

#[derive(Clone, Copy)]
struct AuthorityFacts<'a> {
    caller_id: &'a str,
    target_id: &'a str,
    current_launch: bool,
    anchors_team: bool,
    caller_team: Option<&'a str>,
    target_team: Option<&'a str>,
    live_team: bool,
    destination_allowed: bool,
}

fn tab_mutation_allowed(facts: AuthorityFacts<'_>) -> bool {
    facts.caller_id != facts.target_id
        && facts.current_launch
        && facts.anchors_team
        && facts.caller_team.is_some()
        && facts.caller_team == facts.target_team
        && facts.live_team
        && facts.destination_allowed
}

pub fn authorize_self_promotion(
    mux: &crate::multiplexer::state::MultiplexerState,
    caller: &TabCaller,
    target_id: &str,
) -> Result<(), String> {
    let allowed = caller.tab_id == target_id
        && !caller.binding_nonce.is_empty()
        && mux.get_tab(target_id).is_some_and(|tab| {
            tab.role.is_none()
                && tab.role_profile.is_none()
                && tab.team_id.is_none()
                && tab.parent_tab_id.is_none()
                && tab.neighbor_authority.is_none()
                && tab.ownership.is_none()
                && tab.report_target_mesh_peer.is_none()
                && !tab.protected
                && tab.agent_type.is_some()
                && tab.agent_session_id.is_some()
                && tab.exit_suspected_at.is_none()
                && tab.session_binding_nonce.as_deref() == Some(caller.binding_nonce.as_str())
                && tab.reported_session_binding_nonce == tab.session_binding_nonce
        });
    if allowed {
        Ok(())
    } else {
        Err("Self-promotion requires the same live roleless, team-less local agent session".into())
    }
}

impl ServiceContext {
    pub fn authorize_tab_mutation(
        &self,
        caller: TabMutationCaller<'_>,
        target_id: &str,
        scope: TabMutationScope<'_>,
    ) -> Result<(), String> {
        let TabMutationCaller::Tab(caller) = caller else {
            return Ok(());
        };
        let mux = self.mux.read();
        let caller_tab = mux.get_tab(&caller.tab_id);
        if scope.self_promotion
            && scope.creates_team
            && scope.parent_tab_id.is_none()
            && scope.plan_id.is_none()
            && authorize_self_promotion(&mux, caller, target_id).is_ok()
        {
            return Ok(());
        }
        let caller_team = caller_tab.and_then(|tab| tab.team_id.as_deref());
        let parent_allowed = scope.parent_tab_id.is_none_or(|id| {
            mux.get_tab(id)
                .is_some_and(|tab| tab.team_id.as_deref() == caller_team)
        });
        let plan_allowed = scope.plan_id.is_none_or(|id| {
            self.plan_store
                .get(id)
                .is_some_and(|plan| plan.team_id.as_deref() == caller_team)
        });
        let allowed = tab_mutation_allowed(AuthorityFacts {
            caller_id: &caller.tab_id,
            target_id,
            current_launch: !caller.binding_nonce.is_empty()
                && caller_tab.is_some_and(|tab| {
                    tab.session_binding_nonce.as_deref() == Some(caller.binding_nonce.as_str())
                }),
            anchors_team: caller_tab.is_some_and(|tab| tab.anchors_team()),
            caller_team,
            target_team: mux
                .get_tab(target_id)
                .and_then(|tab| tab.team_id.as_deref()),
            live_team: caller_team.is_some_and(|id| mux.get_team(id).is_some()),
            destination_allowed: !scope.creates_team && parent_allowed && plan_allowed,
        });
        if allowed {
            Ok(())
        } else {
            Err(
                "Tab mutation requires the host or another tab's live same-Team Orchestrator"
                    .into(),
            )
        }
    }
}

#[cfg(test)]
mod tests;
