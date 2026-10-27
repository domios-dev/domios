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
