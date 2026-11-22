use std::collections::{HashMap, HashSet};

use super::super::types::{AgentRole, Tab};
use super::{MultiplexerState, TeamCloseTransaction};

pub const LIFECYCLE_CONTEXT_CONFLICT: &str =
    "lifecycle conflict: tab identity, placement or Team changed; retry by intent";

#[derive(Copy, Clone, Eq, PartialEq, Hash)]
enum TabGroup {
    Children,
    TeamAnchors,
    TeamPlanners,
    PlanWorkers,
}

impl TabGroup {
    fn key(self, tab: &Tab) -> Option<&str> {
        match self {
            Self::Children => tab.parent_tab_id.as_deref(),
            Self::TeamAnchors if tab.anchors_team() => tab.team_id.as_deref(),
            Self::TeamPlanners
                if tab.role == Some(AgentRole::Manager)
                    && tab.role_profile.as_deref()
                        == Some(codeterm_types::agent::role_kind::builtins::PLANNER) =>
            {
                tab.team_id.as_deref()
            }
            Self::PlanWorkers if tab.role == Some(AgentRole::Worker) => {
                tab.task_binding.as_ref()?.plan_id.as_deref()
            }
            _ => None,
        }
    }
}

#[derive(Default)]
struct MembershipRead {
    keys: HashSet<String>,
    expected_count: usize,
}

struct TeamUseRead {
    team: String,
    in_use: bool,
    members: HashSet<String>,
}

type TeamFlag = fn(&MultiplexerState, &str) -> bool;

#[derive(Default)]
pub struct ReadSet {
    tabs: HashSet<String>,
    teams: HashSet<String>,
    workspaces: HashSet<String>,
    groups: HashMap<TabGroup, MembershipRead>,
    team_count: bool,
    team_use: Vec<TeamUseRead>,
    team_flags: Vec<(String, TeamFlag, bool)>,
}

impl ReadSet {
    pub fn tab(&mut self, id: &str) {
        self.tabs.insert(id.into());
    }

    pub fn team(&mut self, id: &str) {
        self.teams.insert(id.into());
    }

    pub fn workspace(&mut self, id: &str) {
        self.workspaces.insert(id.into());
    }

    pub fn team_count(&mut self) {
        self.team_count = true;
    }

    fn select(&mut self, mux: &MultiplexerState, group: TabGroup, key: &str) -> Vec<String> {
        self.groups
            .entry(group)
            .or_default()
            .keys
            .insert(key.into());
        mux.all_tabs()
            .filter(|tab| group.key(tab) == Some(key))
            .map(|tab| tab.id.clone())
            .collect()
    }

    pub fn team_flag(&mut self, mux: &MultiplexerState, id: &str, query: TeamFlag) {
        self.team_flags.push((id.into(), query, query(mux, id)));
    }

    pub fn team_anchors(&mut self, mux: &MultiplexerState, id: &str) -> Vec<String> {
        self.select(mux, TabGroup::TeamAnchors, id)
    }

    pub fn team_planners(&mut self, mux: &MultiplexerState, id: &str) -> Vec<String> {
        self.select(mux, TabGroup::TeamPlanners, id)
    }

    pub fn plan_workers(&mut self, mux: &MultiplexerState, id: &str) -> Vec<String> {
        self.select(mux, TabGroup::PlanWorkers, id)
    }

    pub fn team_in_use(&mut self, mux: &MultiplexerState, team: &str) -> bool {
        let members: HashSet<_> = mux.team_member_tab_ids(team).into_iter().collect();
        let in_use = !members.is_empty();
        self.team_use.push(TeamUseRead {
            team: team.into(),
            in_use,
            members,
        });
        in_use
    }

    pub fn children(&mut self, mux: &MultiplexerState, id: &str) -> Vec<String> {
        self.select(mux, TabGroup::Children, id)
    }

    fn seal(&mut self, base: &MultiplexerState) {
        for (group, read) in &mut self.groups {
            read.expected_count = base
                .all_tabs()
                .filter(|tab| group.key(tab).is_some_and(|key| read.keys.contains(key)))
                .count();
        }
        self.team_use.retain_mut(|read| {
            let original: HashSet<_> = base.team_member_tab_ids(&read.team).into_iter().collect();
            // A staged addition already witnesses use. Otherwise ignore members detached by this transaction.
            if !read.members.is_subset(&original) {
                return false;
            }
            read.members = original.difference(&read.members).cloned().collect();
            true
        });
    }

    fn include_placements(&mut self, mux: &MultiplexerState) {
        for id in &self.tabs {
            if let Some(tab) = mux.get_tab(id) {
                self.teams.extend(tab.team_id.iter().cloned());
            }
            self.workspaces.extend(mux.find_workspace_for_tab(id));
        }
    }

    fn matches(&self, expected: &MultiplexerState, live: &MultiplexerState) -> bool {
        self.tabs_match(expected, live)
            && self.workspaces_match(expected, live)
            && self.teams_match(expected, live)
            && self.groups.iter().all(|(group, read)| {
                let mut count = 0;
                for tab in live.all_tabs() {
                    let Some(key) = group.key(tab).filter(|key| read.keys.contains(*key)) else {
                        continue;
                    };
                    if expected.get_tab(&tab.id).and_then(|old| group.key(old)) != Some(key) {
                        return false;
                    }
                    count += 1;
                }
                count == read.expected_count
            })
    }

    fn tabs_match(&self, expected: &MultiplexerState, live: &MultiplexerState) -> bool {
        self.tabs.iter().all(|id| {
            if expected.tab_close_reservations.contains_key(id)
                || live.tab_close_reservations.contains_key(id)
            {
                return false;
            }
            match (expected.get_tab(id), live.get_tab(id)) {
                (Some(a), Some(b)) => matches(a, b),
                (None, None) => true,
                _ => false,
            }
        })
    }

    fn teams_match(&self, expected: &MultiplexerState, live: &MultiplexerState) -> bool {
        if self.team_count && expected.team_count() != live.team_count() {
            return false;
        }
        if self.teams.len() > 1 {
            let selected = |id: &&String| self.teams.contains(*id);
            if !expected
                .team_order
                .iter()
                .filter(selected)
                .eq(live.team_order.iter().filter(selected))
            {
                return false;
            }
        }
        self.teams
            .iter()
            .all(|id| expected.get_team(id) == live.get_team(id))
            && self
                .team_flags
                .iter()
                .all(|(id, query, value)| query(live, id) == *value)
            && self.team_use.iter().all(|read| {
                read.in_use
                    == live.all_tabs().any(|tab| {
                        tab.team_id.as_deref() == Some(read.team.as_str())
                            && !read.members.contains(&tab.id)
                    })
            })
    }

    fn workspaces_match(&self, expected: &MultiplexerState, live: &MultiplexerState) -> bool {
        if self.workspaces.len() > 1 {
            let ids = |ws: &super::super::types::Workspace| self.workspaces.contains(&ws.id);
            let a = expected
                .workspaces
                .iter()
                .filter(|ws| ids(ws))
                .map(|ws| &ws.id);
            let b = live.workspaces.iter().filter(|ws| ids(ws)).map(|ws| &ws.id);
            if !a.eq(b) {
                return false;
            }
        }
        self.workspaces.iter().all(|id| {
            if expected.workspace_close_reservations.contains_key(id)
                || live.workspace_close_reservations.contains_key(id)
            {
                return false;
            }
            let selected = |tab: &&String| self.tabs.contains(*tab);
            match (expected.get_workspace(id), live.get_workspace(id)) {
                (Some(a), Some(b)) => {
                    a.protected == b.protected
                        && a.locked == b.locked
                        && a.created_at_ms == b.created_at_ms
                        && a.tabs
                            .iter()
                            .filter(selected)
                            .eq(b.tabs.iter().filter(selected))
                }
                (None, None) => true,
                _ => false,
            }
        })
    }
}

pub struct LifecyclePatch {
    before: TeamCloseTransaction,
    after: TeamCloseTransaction,
    reads: ReadSet,
    written_tabs: Vec<String>,
    written_teams: HashSet<String>,
}

impl LifecyclePatch {
    pub fn new(
        before: TeamCloseTransaction,
        after: TeamCloseTransaction,
        mut reads: ReadSet,
    ) -> Self {
        let base = &before.staged;
        let staged = &after.staged;
        let written_tabs: Vec<_> = staged
            .tabs
            .iter()
            .filter(|(id, tab)| {
                base.tabs
                    .get(*id)
                    .is_some_and(|old| !matches_patch(old, tab))
            })
            .map(|(id, _)| id.clone())
            .collect();
        let written_teams: HashSet<_> = base
            .teams
            .keys()
            .chain(staged.teams.keys())
            .filter(|id| base.get_team(id) != staged.get_team(id))
            .cloned()
            .collect();
        reads.tabs.extend(written_tabs.iter().cloned());
        reads.teams.extend(written_teams.iter().cloned());
        for id in &written_tabs {
            for tab in [base.get_tab(id), staged.get_tab(id)].into_iter().flatten() {
                reads.tabs.extend(tab.parent_tab_id.iter().cloned());
            }
        }
        reads.seal(base);
        reads.include_placements(base);
        reads.include_placements(staged);
        Self {
            before,
            after,
            reads,
            written_tabs,
            written_teams,
        }
    }

    pub fn apply(&self, target: &mut MultiplexerState) -> Result<(), String> {
        if !self.reads.matches(&self.before.staged, target)
            || !self.can_remove_teams(&self.before.staged, &self.after.staged, target)
        {
            return Err(LIFECYCLE_CONTEXT_CONFLICT.into());
        }
        self.write(&self.after.staged, target, false);
        Ok(())
    }

    pub fn revert(&self, target: &mut MultiplexerState) -> Result<(), String> {
        let staged = &self.after.staged;
        for id in &self.written_tabs {
            let expected = &staged.tabs[id];
            let original = &self.before.staged.tabs[id];
            if let Some(parent) = original
                .parent_tab_id
                .as_deref()
                .filter(|_| original.parent_tab_id != expected.parent_tab_id)
            {
                if let Some(parent_before) = staged.get_tab(parent) {
                    if target.get_tab(parent).is_none_or(|live| {
                        !same_launch(parent_before, live) || !matches_patch(parent_before, live)
                    }) || target.ensure_tab_membership_mutable(parent).is_err()
                    {
                        return Err("lifecycle rollback conflict: original parent changed".into());
                    }
                }
            }
            if let Some(team) = original
                .team_id
                .as_deref()
                .filter(|_| original.team_id != expected.team_id)
            {
                if staged.get_team(team).map(|t| t.created_at)
                    != target.get_team(team).map(|t| t.created_at)
                {
                    return Err("lifecycle rollback conflict: original Team changed".into());
                }
            }
            if target.get_tab(id).is_none_or(|current| {
                !matches_patch(expected, current) || !same_launch(expected, current)
            }) || staged.find_workspace_for_tab(id) != target.find_workspace_for_tab(id)
                || target.ensure_tab_membership_mutable(id).is_err()
            {
                return Err("lifecycle rollback conflict: a patched tab changed".into());
            }
        }
        if self
            .written_teams
            .iter()
            .any(|id| staged.get_team(id) != target.get_team(id))
            || !self.can_remove_teams(staged, &self.before.staged, target)
        {
            return Err("lifecycle rollback conflict: a patched Team changed".into());
        }
        self.write(&self.before.staged, target, true);
        Ok(())
    }

    fn can_remove_teams(
        &self,
        from: &MultiplexerState,
        to: &MultiplexerState,
        live: &MultiplexerState,
    ) -> bool {
        self.written_teams
            .iter()
            .filter(|id| to.get_team(id).is_none())
            .all(|id| from.team_member_tab_ids(id) == live.team_member_tab_ids(id))
    }

    fn write(&self, staged: &MultiplexerState, target: &mut MultiplexerState, reverting: bool) {
        for id in &self.written_tabs {
            let current = target.tabs.get_mut(id).expect("validated lifecycle tab");
            let staged = &staged.tabs[id];
            current.role = staged.role;
            current.role_profile = staged.role_profile.clone();
            current.team_id = staged.team_id.clone();
            current.parent_tab_id = staged.parent_tab_id.clone();
            current.can_spawn = staged.can_spawn;
            current.depth = staged.depth;
            current.pending_paired_planner_tab_id = staged.pending_paired_planner_tab_id.clone();
            current.report_target_tab_id = staged.report_target_tab_id.clone();
            current.report_target_mesh_peer = staged.report_target_mesh_peer.clone();
            current.spawned_by_tab_id = staged.spawned_by_tab_id.clone();
        }

        if self.written_teams.is_empty() {
            return;
        }
        for id in &self.written_teams {
            match staged.get_team(id) {
                Some(team) => {
                    target.teams.insert(id.clone(), team.clone());
                }
                None => {
                    target.remove_team(id);
                }
            }
        }
        // Restore a removed Team beside its surviving successor while preserving live insertions.
        for (i, id) in staged.team_order.iter().enumerate().rev() {
            if self.written_teams.contains(id) && !target.team_order.contains(id) {
                let index = staged.team_order[i + 1..]
                    .iter()
                    .find_map(|next| target.team_order.iter().position(|live| live == next))
                    .unwrap_or_else(|| {
                        if reverting {
                            staged.team_order[..i]
                                .iter()
                                .rev()
                                .find_map(|prev| {
                                    target
                                        .team_order
                                        .iter()
                                        .position(|live| live == prev)
                                        .map(|n| n + 1)
                                })
                                .unwrap_or(0)
                        } else {
                            target.team_order.len()
                        }
                    });
                target.team_order.insert(index, id.clone());
            }
        }
    }
}

fn same_launch(a: &Tab, b: &Tab) -> bool {
    a.id == b.id
        && a.created_at_ms == b.created_at_ms
        && a.agent_type == b.agent_type
        && a.agent_session_id == b.agent_session_id
        && a.session_binding_nonce == b.session_binding_nonce
        && a.reported_session_binding_nonce == b.reported_session_binding_nonce
        && a.lifecycle_epoch == b.lifecycle_epoch
        && a.lifecycle_operation_id == b.lifecycle_operation_id
        && a.pty_id == b.pty_id
        && a.backing == b.backing
}

fn matches(a: &Tab, b: &Tab) -> bool {
    matches_patch(a, b)
        && same_launch(a, b)
        && a.ownership == b.ownership
        && a.neighbor_authority == b.neighbor_authority
        && a.agent_booting == b.agent_booting
        && a.frozen == b.frozen
        && a.locked == b.locked
        && a.protected == b.protected
        && a.boot_stage == b.boot_stage
        && a.title == b.title
        && a.chat_view == b.chat_view
        && a.exit_suspected_at == b.exit_suspected_at
        && a.task_binding == b.task_binding
        && a.pending_delivery.is_some() == b.pending_delivery.is_some()
        && a.agent_runtime_state.is_some() == b.agent_runtime_state.is_some()
        && a.restorable_launch.account_id == b.restorable_launch.account_id
        && a.cwd == b.cwd
}

#[cfg(test)]
mod tests;

fn matches_patch(a: &Tab, b: &Tab) -> bool {
    a.role == b.role
        && a.role_profile == b.role_profile
        && a.team_id == b.team_id
        && a.parent_tab_id == b.parent_tab_id
        && a.can_spawn == b.can_spawn
        && a.depth == b.depth
        && a.pending_paired_planner_tab_id == b.pending_paired_planner_tab_id
        && a.report_target_tab_id == b.report_target_tab_id
        && a.report_target_mesh_peer == b.report_target_mesh_peer
        && a.spawned_by_tab_id == b.spawned_by_tab_id
}
