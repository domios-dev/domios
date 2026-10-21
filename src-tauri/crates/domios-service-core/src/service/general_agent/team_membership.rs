//! The structural control behind the General Agent's "you coordinate Teams,
//! you never join one" rule. Every Team membership mutation calls
//! [`refuse_general_agent_team_membership`], so no transport can reach a
//! membership write without passing it.

use codeterm_types::team::general_agent_membership::{
    CorrectiveShape, GeneralAgentTeamRefusal, MembershipAttempt,
};

use crate::multiplexer::state::MultiplexerState;

use super::identity::{is_general_agent, GENERAL_AGENT_KIND};

/// Refuse any Team mutation that would make a General-Agent-kind tab a member,
/// or that reads such a tab's membership as its authority. Identity comes from
/// the tab's own role kind — never a tab id, never a provider.
///
/// An unknown or blank tab is not this guard's business: the mutation's own
/// lookup reports it, and inventing a membership refusal for a missing tab
/// would teach the wrong lesson.
pub fn refuse_general_agent_team_membership(
    mux: &MultiplexerState,
    tab_id: &str,
    attempt: MembershipAttempt,
    shape: &CorrectiveShape,
) -> Result<(), GeneralAgentTeamRefusal> {
    let tab_id = tab_id.trim();
    if tab_id.is_empty() {
        return Ok(());
    }
    let Some(tab) = mux.get_tab(tab_id) else {
        return Ok(());
    };
    if !is_general_agent(tab) {
        return Ok(());
    }
    Err(GeneralAgentTeamRefusal::new(
        tab_id,
        tab.role_kind_id().unwrap_or(GENERAL_AGENT_KIND),
        attempt,
        shape.clone(),
    ))
}

/// Role placement mints the parent's Team onto the parent, so parenting any tab
/// under the General Agent would enrol it. One named entry for that shape; the
/// refusal is still decided by the single guard above.
pub fn refuse_parenting_under_general_agent(
    mux: &MultiplexerState,
    parent_tab_id: Option<&str>,
) -> Result<(), GeneralAgentTeamRefusal> {
    let Some(parent_tab_id) = parent_tab_id else {
        return Ok(());
    };
    let shape = shape_for_tab(mux, parent_tab_id);
    refuse_general_agent_team_membership(mux, parent_tab_id, MembershipAttempt::Join, &shape)
}

/// The corrective shape a Team mutation can offer without knowing which
/// providers were requested: the target workspace, providers left as
/// placeholders.
pub fn shape_for_tab(mux: &MultiplexerState, tab_id: &str) -> CorrectiveShape {
    CorrectiveShape::in_workspace(mux.find_workspace_for_tab(tab_id).as_deref())
}

#[cfg(test)]
mod tests;
