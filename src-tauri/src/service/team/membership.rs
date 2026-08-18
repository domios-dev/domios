use codeterm_types::team::Team;

use crate::multiplexer::state::MultiplexerState;

/// Placement one Team attachment resolves to: the Team, the member tab that
/// asked for it, and the Team's live anchor the attached tab hangs under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TeamAttachment {
    pub team_id: String,
    pub caller_tab_id: String,
    pub manager_tab_id: String,
}

/// Membership authorization shared by every "attach a tab to a Team" verb —
/// `tab new --team`, `agent spawn --team`, and `team adopt-agent`. The caller
/// must already be a member; nothing here reads a provider id.
pub(crate) fn authorize_team_attachment(
    mux: &MultiplexerState,
    team_id: &str,
    caller_tab_id: &str,
    workspace_id: &str,
) -> Result<TeamAttachment, String> {
    let caller = mux
        .get_tab(caller_tab_id)
        .ok_or_else(|| format!("Creator Tab {caller_tab_id} not found"))?;
    if caller.team_id.as_deref() != Some(team_id) {
        return Err(format!(
            "Creator Tab {caller_tab_id} does not belong to Team {team_id}"
        ));
    }
    let team_workspace = mux
        .team_workspace(team_id)
        .ok_or_else(|| format!("Team {team_id} not found"))?;
    if team_workspace != workspace_id {
        return Err(format!(
            "Team {team_id} belongs to workspace {team_workspace}, not {workspace_id}"
        ));
    }
    let manager_tab_id = mux
        .snapshot()
        .tabs
        .values()
        .find(|tab| tab.team_id.as_deref() == Some(team_id) && tab.anchors_team())
        .map(|tab| tab.id.clone())
        .ok_or_else(|| format!("Team {team_id} has no live Orchestrator"))?;
    Ok(TeamAttachment {
        team_id: team_id.to_string(),
        caller_tab_id: caller_tab_id.to_string(),
        manager_tab_id,
    })
}

/// Write the resolved placement onto an already-created tab.
pub(crate) fn apply_team_attachment(
    mux: &mut MultiplexerState,
    attachment: &TeamAttachment,
    tab_id: &str,
) -> Result<(), String> {
    let tab = mux
        .get_tab_mut(tab_id)
        .ok_or_else(|| format!("Tab {tab_id} not found"))?;
    tab.team_id = Some(attachment.team_id.clone());
    tab.spawned_by_tab_id = Some(attachment.caller_tab_id.clone());
    tab.parent_tab_id = Some(attachment.manager_tab_id.clone());
    tab.depth = 2;
    Ok(())
}

/// Resolve the spawn-time `--team` pair. Both halves travel together: a Team
/// without its calling member would be an unauthorized attach.
pub(crate) fn resolve_spawn_attachment(
    mux: &MultiplexerState,
    team_id: Option<&str>,
    creator_tab_id: Option<&str>,
    workspace_id: &str,
) -> Result<Option<TeamAttachment>, String> {
    match (team_id, creator_tab_id) {
        (None, _) => Ok(None),
        (Some(team_id), Some(creator_tab_id)) => {
            authorize_team_attachment(mux, team_id, creator_tab_id, workspace_id).map(Some)
        }
        (Some(_), None) => {
            Err("team_id and creator_tab_id are required together for a Team attach".into())
        }
    }
}

/// Attach an already-running roleless agent tab to a Team. A role-bearing tab
/// already draws its Team from its own hierarchy binding, so it is refused
/// rather than silently re-parented.
pub(crate) fn adopt_agent_into_team(
    mux: &mut MultiplexerState,
    team_id: &str,
    tab_id: &str,
    caller_tab_id: &str,
) -> Result<Team, String> {
    let tab = mux
        .get_tab(tab_id)
        .ok_or_else(|| format!("Tab {tab_id} not found"))?;
    if let Some(existing) = tab.team_id.as_deref() {
        return Err(format!(
            "Tab {tab_id} already belongs to Team {existing}; a tab joins exactly one Team"
        ));
    }
    if let Some(role) = tab.role {
        return Err(format!(
            "Tab {tab_id} holds role {}; only a roleless agent tab can be adopted into a Team",
            role.label()
        ));
    }
    let workspace_id = mux
        .find_workspace_for_tab(tab_id)
        .ok_or_else(|| format!("Tab {tab_id} has no workspace"))?;
    let attachment = authorize_team_attachment(mux, team_id, caller_tab_id, &workspace_id)?;
    if attachment.manager_tab_id == tab_id {
        return Err(format!("Tab {tab_id} anchors Team {team_id} already"));
    }
    apply_team_attachment(mux, &attachment, tab_id)?;
    mux.get_team(team_id)
        .cloned()
        .ok_or_else(|| format!("Team {team_id} not found"))
}

#[cfg(test)]
mod tests;
