use super::super::types::Tab;
use super::{MultiplexerState, TeamCloseTransaction};

pub const LIFECYCLE_CONTEXT_CONFLICT: &str =
    "lifecycle conflict: tab identity, placement or Team changed; retry by intent";

pub struct LifecyclePatch {
    before: TeamCloseTransaction,
    after: TeamCloseTransaction,
}

impl LifecyclePatch {
    pub fn new(before: TeamCloseTransaction, after: TeamCloseTransaction) -> Self {
        Self { before, after }
    }

    pub fn apply(&self, target: &mut MultiplexerState) -> Result<(), String> {
        apply(&self.before.staged, &self.after.staged, target, true)
    }

    pub fn revert(&self, target: &mut MultiplexerState) -> Result<(), String> {
        apply(&self.after.staged, &self.before.staged, target, false)
    }
}

fn matches(a: &Tab, b: &Tab) -> bool {
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
        && a.agent_type == b.agent_type
        && a.agent_session_id == b.agent_session_id
        && a.session_binding_nonce == b.session_binding_nonce
        && a.reported_session_binding_nonce == b.reported_session_binding_nonce
        && a.ownership == b.ownership
        && a.neighbor_authority == b.neighbor_authority
        && a.lifecycle_epoch == b.lifecycle_epoch
        && a.lifecycle_operation_id == b.lifecycle_operation_id
        && a.agent_booting == b.agent_booting
        && a.frozen == b.frozen
        && a.locked == b.locked
        && a.pty_id == b.pty_id
        && a.backing == b.backing
        && a.boot_stage == b.boot_stage
        && a.title == b.title
        && a.chat_view == b.chat_view
        && a.exit_suspected_at == b.exit_suspected_at
        && a.task_binding == b.task_binding
}

fn apply(
    before: &MultiplexerState,
    after: &MultiplexerState,
    target: &mut MultiplexerState,
    validate_context: bool,
) -> Result<(), String> {
    if validate_context
        && (before.tabs.len() != target.tabs.len()
            || before.workspaces.len() != target.workspaces.len()
            || before
                .workspaces
                .iter()
                .zip(&target.workspaces)
                .any(|(a, b)| {
                    a.id != b.id
                        || a.tabs != b.tabs
                        || a.protected != b.protected
                        || a.locked != b.locked
                })
            || before.teams != target.teams
            || before.team_order != target.team_order
            || before.workspace_close_reservations != target.workspace_close_reservations
            || before.tab_close_reservations != target.tab_close_reservations
            || before.tabs.iter().any(|(id, tab)| {
                target
                    .tabs
                    .get(id)
                    .is_none_or(|current| !matches(tab, current))
            }))
    {
        return Err(LIFECYCLE_CONTEXT_CONFLICT.into());
    }
    for (id, staged) in &after.tabs {
        let expected = &before.tabs[id];
        let changed = !matches_patch(expected, staged);
        if changed
            && target
                .tabs
                .get(id)
                .is_none_or(|current| !matches_patch(expected, current))
        {
            return Err("lifecycle rollback conflict: a patched tab changed".into());
        }
    }
    if before.teams != after.teams && target.teams != before.teams {
        return Err("lifecycle rollback conflict: Teams changed".into());
    }
    for (id, staged) in &after.tabs {
        let expected = &before.tabs[id];
        if matches_patch(expected, staged) {
            continue;
        }
        let Some(current) = target.tabs.get_mut(id) else {
            continue;
        };

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
    if before.teams != after.teams {
        target.teams = after.teams.clone();
        target.team_order = after.team_order.clone();
    }
    Ok(())
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
