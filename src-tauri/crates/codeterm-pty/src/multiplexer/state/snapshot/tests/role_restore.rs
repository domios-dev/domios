use super::snapshot_with;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::TabAudience;
use crate::session::store::SplitNodeSnapshot;

/// A launch that never completed persists a role with no provider identity.
fn partial_launch_residue_leaf(role: &str) -> SplitNodeSnapshot {
    SplitNodeSnapshot::Leaf {
        tab_id: "db79d2da".into(),
        cwd: None,
        title: Some("Plan card fixture".into()),
        description: None,
        title_level: Some("User".into()),
        agent_type: None,
        agent_type_locked: false,
        claude_session_id: None,
        agent_identity: None,
        role: Some(role.into()),
        role_profile: None,
        team_id: Some("team_residue".into()),
        parent_tab_id: None,
        spawned_by_tab_id: None,
        report_target_tab_id: None,
        report_target_mesh_peer: None,
        ownership: None,
        can_spawn: true,
        depth: 1,
        chat_view: Some(false),
        auto_approve: false,
        audience: TabAudience::User,
        agent_session_model: None,
        agent_planned_model: None,
        agent_requested_model: None,
        agent_provider_resolved_model: None,
        model_fallback: None,
        model_identity_mismatch: None,
        chatbackend_plugin_id: None,
        chatbackend_session_id: None,
        watcher: None,
        pending_delivery: None,
    }
}

#[test]
fn partial_launch_residue_never_restores_as_a_live_orchestrator() {
    for role in ["Orchestrator", "Conductor", "Manager", "Worker"] {
        let restored = MultiplexerState::from_session_snapshot(&snapshot_with(
            partial_launch_residue_leaf(role),
        ));
        let tab = restored.get_tab("db79d2da").expect("residue tab survives");

        assert_eq!(tab.role, None, "{role}: no provider identity, no role");
        assert_eq!(tab.role_profile, None, "{role}");
        assert_eq!(tab.team_id, None, "{role}: Team metadata rolls back too");
        assert_eq!(tab.parent_tab_id, None, "{role}");
        assert!(!tab.can_spawn, "{role}");
        assert_eq!(tab.depth, 0, "{role}");
        assert!(!tab.anchors_team(), "{role}");
        assert!(!tab.has_team_orchestrator_authority(), "{role}");
    }
}

/// A Team utility shell never claims a role, so it is not residue: it keeps the
/// membership that makes Team close responsible for cleaning it up.
#[test]
fn a_roleless_team_utility_tab_keeps_its_team_across_restore() {
    let mut leaf = partial_launch_residue_leaf("Worker");
    if let SplitNodeSnapshot::Leaf {
        ref mut role,
        ref mut can_spawn,
        ref mut depth,
        ..
    } = leaf
    {
        *role = None;
        *can_spawn = false;
        *depth = 0;
    }

    let restored = MultiplexerState::from_session_snapshot(&snapshot_with(leaf));
    let tab = restored.get_tab("db79d2da").expect("utility tab survives");

    assert_eq!(tab.role, None);
    assert_eq!(tab.team_id.as_deref(), Some("team_residue"));
    assert!(!tab.has_team_authority());
    assert!(!tab.has_team_orchestrator_authority());
    assert!(!tab.can_spawn);
}

/// The title is not evidence of a provider — an incomplete tab must never be
/// handed a guessed one just because its name mentions an agent.
#[test]
fn residue_titled_like_an_agent_still_loses_its_role() {
    let mut leaf = partial_launch_residue_leaf("Orchestrator");
    if let SplitNodeSnapshot::Leaf { ref mut title, .. } = leaf {
        *title = Some("codex orchestrator".into());
    }

    let restored = MultiplexerState::from_session_snapshot(&snapshot_with(leaf));
    let tab = restored.get_tab("db79d2da").expect("residue tab survives");

    assert_eq!(tab.role, None, "a title never resurrects Team authority");
    assert!(!tab.can_spawn);
}

/// A complete agent tab keeps everything — the guard must not demote real work.
#[test]
fn a_complete_agent_tab_keeps_its_restored_role_and_team() {
    let mut leaf = partial_launch_residue_leaf("Orchestrator");
    if let SplitNodeSnapshot::Leaf {
        ref mut agent_type, ..
    } = leaf
    {
        *agent_type = Some("codex".into());
    }

    let restored = MultiplexerState::from_session_snapshot(&snapshot_with(leaf));
    let tab = restored.get_tab("db79d2da").expect("agent tab");

    assert_eq!(
        tab.role,
        Some(crate::multiplexer::types::AgentRole::Manager)
    );
    assert_eq!(tab.role_profile.as_deref(), Some("orchestrator"));
    assert_eq!(tab.team_id.as_deref(), Some("team_residue"));
    assert!(tab.can_spawn);
    assert_eq!(tab.depth, 1);
    assert!(tab.anchors_team());
}
