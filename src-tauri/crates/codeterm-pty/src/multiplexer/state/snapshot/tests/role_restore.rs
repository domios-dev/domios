use super::snapshot_with;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{AgentRole, TabAudience};
use crate::session::store::SplitNodeSnapshot;

/// A persisted role claim, with or without a provider identity behind it.
fn role_claim_leaf(role: &str) -> SplitNodeSnapshot {
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
        permission_auto_confirm: false,
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

/// Restore is unconditional: a role claim survives verbatim even when no
/// provider identity was persisted alongside it.
#[test]
fn a_role_claim_survives_restore_without_provider_identity() {
    for (role, expected, expected_profile) in [
        ("Orchestrator", AgentRole::Manager, Some("orchestrator")),
        ("Conductor", AgentRole::Manager, Some("orchestrator")),
        ("Worker", AgentRole::Worker, None),
    ] {
        let restored =
            MultiplexerState::from_session_snapshot(&snapshot_with(role_claim_leaf(role)));
        let tab = restored.get_tab("db79d2da").expect("tab survives");

        assert_eq!(tab.role, Some(expected), "{role}");
        assert_eq!(tab.role_profile.as_deref(), expected_profile, "{role}");
        assert_eq!(tab.team_id.as_deref(), Some("team_residue"), "{role}");
        assert!(tab.can_spawn, "{role}");
        assert_eq!(tab.depth, 1, "{role}");
        assert_eq!(tab.agent_type, None, "{role}: no provider is invented");
    }
}

/// A roleless Team utility shell keeps the membership that makes Team close
/// responsible for cleaning it up.
#[test]
fn a_roleless_team_utility_tab_keeps_its_team_across_restore() {
    let mut leaf = role_claim_leaf("Worker");
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

/// The title is never evidence of a provider — restore keeps the persisted
/// role but never invents an agent_type from the tab's name.
#[test]
fn a_title_never_invents_a_provider_identity() {
    let mut leaf = role_claim_leaf("Orchestrator");
    if let SplitNodeSnapshot::Leaf { ref mut title, .. } = leaf {
        *title = Some("codex orchestrator".into());
    }

    let restored = MultiplexerState::from_session_snapshot(&snapshot_with(leaf));
    let tab = restored.get_tab("db79d2da").expect("tab survives");

    assert_eq!(tab.agent_type, None, "a title never guesses a provider");
    assert_eq!(
        tab.role,
        Some(AgentRole::Manager),
        "the role claim survives"
    );
    assert!(tab.can_spawn);
}

/// A complete agent tab keeps everything.
#[test]
fn a_complete_agent_tab_keeps_its_restored_role_and_team() {
    let mut leaf = role_claim_leaf("Orchestrator");
    if let SplitNodeSnapshot::Leaf {
        ref mut agent_type, ..
    } = leaf
    {
        *agent_type = Some("codex".into());
    }

    let restored = MultiplexerState::from_session_snapshot(&snapshot_with(leaf));
    let tab = restored.get_tab("db79d2da").expect("agent tab");

    assert_eq!(tab.role, Some(AgentRole::Manager));
    assert_eq!(tab.role_profile.as_deref(), Some("orchestrator"));
    assert_eq!(tab.team_id.as_deref(), Some("team_residue"));
    assert!(tab.can_spawn);
    assert_eq!(tab.depth, 1);
    assert!(tab.anchors_team());
}

#[test]
fn tab_name_contract_restore_preserves_task_executor_name() {
    let mut leaf = role_claim_leaf("Worker");
    if let SplitNodeSnapshot::Leaf { ref mut title, .. } = leaf {
        *title = Some("Ada".into());
    }

    let restored = MultiplexerState::from_session_snapshot(&snapshot_with(leaf));
    assert_eq!(
        restored.get_tab("db79d2da").unwrap().title,
        "Ada",
        "restore keeps the persisted executor name exactly"
    );
}
