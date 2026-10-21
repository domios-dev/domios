use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn observed_agents(
    svc: &ServiceContext,
    plan: &Plan,
    now: u64,
) -> BTreeMap<String, crate::plan::CuratorAgentState> {
    let tabs = svc
        .mux
        .read()
        .tabs
        .values()
        .filter(|tab| tab.team_id == plan.team_id && tab.agent_type.is_some())
        .cloned()
        .collect::<Vec<_>>();
    tabs.into_iter()
        .map(|tab| {
            let state = svc
                .resolved_tab_status(&tab)
                .map(|status| status.label())
                .unwrap_or("unknown")
                .to_string();
            let since = plan
                .control_loop
                .agent_states
                .get(&tab.id)
                .filter(|prior| prior.state == state)
                .map_or(now, |prior| prior.since);
            (tab.id, crate::plan::CuratorAgentState { state, since })
        })
        .collect()
}

pub(super) fn append(
    svc: &ServiceContext,
    plan: &Plan,
    since: u64,
    now: u64,
    rows: &mut BTreeMap<String, Vec<digest::Row>>,
) -> Result<(), String> {
    rows.extend(curator::movement::rows(plan, since, now));
    for (id, state) in &plan.control_loop.agent_states {
        rows.entry("agent capacity".into())
            .or_default()
            .push(digest::Row {
                id: id.clone(),
                kind: "agent".into(),
                state: state.state.clone(),
                summary: format!(
                    "observed_minutes={}",
                    now.saturating_sub(state.since) / 60_000
                ),
            });
    }
    let team_tabs = svc
        .mux
        .read()
        .tabs
        .values()
        .filter(|tab| tab.team_id == plan.team_id)
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
        .cloned()
        .collect::<Vec<_>>();
    for notice in &notifications {
        if notice.level != Some(codeterm_notification_core::contract::NotifyLevel::ActionNeeded) {
            continue;
        }
        let answered = notice
            .answer
            .as_ref()
            .is_some_and(|answer| answer.answered.is_some());
        if !answered && !notice.resolved {
            rows.entry("owner questions".into())
                .or_default()
                .push(digest::Row {
                    id: notice.id.clone(),
                    kind: "question".into(),
                    state: "open".into(),
                    summary: now.saturating_sub(notice.timestamp).to_string(),
                });
        } else if answered
            && !plan
                .control_loop
                .observed_owner_answers
                .contains(&notice.id)
            && (plan.control_loop.last_round.is_some() || notice.timestamp > since)
        {
            rows.entry("decisions".into())
                .or_default()
                .push(digest::Row {
                    id: notice.id.clone(),
                    kind: "owner_answer".into(),
                    state: "newly_observed".into(),
                    summary: now.to_string(),
                });
        }
    }
    let answers = notifications
        .iter()
        .filter(|notice| {
            notice
                .answer
                .as_ref()
                .is_some_and(|answer| answer.answered.is_some())
        })
        .map(|notice| notice.id.clone())
        .collect();
    svc.plan_store
        .with_plan(&plan.id, now, |plan| {
            plan.control_loop.observed_owner_answers = answers;
            Ok(())
        })
        .map_err(|e| e.to_string())?;
    for task in plan.tasks.iter().filter(|task| {
        task.status == crate::plan::TaskStatus::Done && task.status_changed_at > since
    }) {
        let notice = notifications.iter().any(|notice| {
            notice.timestamp >= task.status_changed_at
                && (notice.title.contains(&task.id)
                    || notice
                        .body
                        .as_ref()
                        .is_some_and(|body| body.contains(&task.id)))
        });
        let shown = plan.strides.iter().any(|stride| {
            stride.ts >= task.status_changed_at
                && matches!(stride.kind.as_str(), "owner_notice" | "shown")
                && stride.links.iter().any(|link| link == &task.id)
        });
        if !notice && !shown {
            rows.entry("done but not shown".into())
                .or_default()
                .push(digest::Row {
                    id: task.id.clone(),
                    kind: "accepted".into(),
                    state: "notice_unconfirmed".into(),
                    summary: task.status_changed_at.to_string(),
                });
        }
    }
    Ok(())
}
