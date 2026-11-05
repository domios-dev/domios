use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn observed_agents(
    svc: &ServiceContext,
    team_id: &Option<String>,
    prior: &BTreeMap<String, crate::plan::CuratorAgentState>,
    now: u64,
) -> BTreeMap<String, crate::plan::CuratorAgentState> {
    let tabs = svc
        .mux
        .read()
        .all_tabs()
        .filter(|tab| &tab.team_id == team_id && tab.agent_type.is_some())
        .cloned()
        .collect::<Vec<_>>();
    tabs.into_iter()
        .map(|tab| {
            let state = svc
                .resolved_tab_status(&tab)
                .map(|status| status.label())
                .unwrap_or("unknown")
                .to_string();
            let since = prior
                .get(&tab.id)
                .filter(|prior| prior.state == state)
                .map_or(now, |prior| prior.since);
            (tab.id, crate::plan::CuratorAgentState { state, since })
        })
        .collect()
}

pub(super) fn capture_notices(
    svc: &ServiceContext,
    team_id: &Option<String>,
) -> Vec<codeterm_plan::service_plan::CuratorRuntimeNotice> {
    let team_tabs = svc
        .mux
        .read()
        .all_tabs()
        .filter(|tab| &tab.team_id == team_id)
        .map(|tab| tab.id.clone())
        .collect::<BTreeSet<_>>();
    let notifications = svc
        .notif
        .lock()
        .list(false)
        .into_iter()
        .filter(|notice| {
            notice
                .tab_id
                .as_ref()
                .is_some_and(|id| team_tabs.contains(id))
        })
        .map(|notice| codeterm_plan::service_plan::CuratorRuntimeNotice {
            id: notice.id.clone(),
            timestamp: notice.timestamp,
            action_needed: notice.level
                == Some(codeterm_notification_core::contract::NotifyLevel::ActionNeeded),
            answered: notice
                .answer
                .as_ref()
                .is_some_and(|answer| answer.answered.is_some()),
            resolved: notice.resolved,
            title: notice.title.clone(),
            body: notice.body.clone(),
        })
        .collect::<Vec<_>>();
    notifications
}
