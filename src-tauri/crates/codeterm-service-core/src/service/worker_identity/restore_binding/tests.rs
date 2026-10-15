use super::*;
use crate::multiplexer::state::MultiplexerState;

fn entry(
    session_id: Option<&str>,
    resume_target_session_id: Option<&str>,
    nonce: &str,
) -> PriorBindingNonce {
    PriorBindingNonce {
        session_id: session_id.map(str::to_string),
        resume_target_session_id: resume_target_session_id.map(str::to_string),
        nonce: nonce.into(),
    }
}

#[test]
fn attempt_binding_restore_current_tab_role_requires_a_worker_leaf_kind() {
    let (mut mux, parent_tab_id) = MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let tab_id = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role(&tab_id, AgentRole::Worker, Some(&parent_tab_id))
        .unwrap();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    assert_eq!(tab.role, Some(AgentRole::Worker));
    assert_eq!(tab.role_profile, None);
    assert!(is_current_worker_role(tab));

    tab.role_profile = Some("worker".into());
    assert!(is_current_worker_role(tab));

    tab.role_profile = Some("custom-leaf".into());
    assert!(is_current_worker_role(tab));

    tab.role_profile = Some("planner".into());
    assert!(!is_current_worker_role(tab));

    tab.role_profile = Some("orchestrator".into());
    assert!(!is_current_worker_role(tab));

    tab.role = Some(AgentRole::Manager);
    tab.role_profile = Some("planner".into());
    assert!(!is_current_worker_role(tab));
}

#[test]
fn attempt_binding_restore_same_session_lineage_accepts_the_ordered_retained_suffix() {
    let lineage = [
        entry(
            Some("earlier-session"),
            None,
            "history-before-retained-entry",
        ),
        entry(Some("session-a"), None, "nonce-old"),
        entry(None, Some("session-a"), "resume-intent"),
        entry(Some("session-a"), None, "nonce-intermediate"),
    ];
    let proof = prove_same_session_restoration(&lineage, "nonce-old", "nonce-current", "session-a")
        .expect("the suffix is ordered and belongs to the authenticated session");
    assert!(proof.digest.starts_with("sha256:"));
}

#[test]
fn attempt_binding_restore_same_session_lineage_refuses_unknown_ambiguous_foreign_and_unqualified_entries(
) {
    let invalid = [
        vec![entry(Some("session-a"), None, "different-old")],
        vec![entry(None, Some("session-a"), "nonce-old")],
        vec![
            entry(Some("session-a"), None, "nonce-old"),
            entry(Some("session-b"), None, "foreign-session"),
        ],
        vec![
            entry(Some("session-a"), None, "nonce-old"),
            entry(None, None, "unqualified-gap"),
        ],
        vec![
            entry(Some("session-a"), None, "nonce-old"),
            entry(Some("session-a"), None, "nonce-old"),
        ],
        vec![
            entry(Some("session-a"), None, "nonce-old"),
            entry(Some("session-a"), None, "nonce-current"),
        ],
    ];
    for lineage in invalid {
        assert!(prove_same_session_restoration(
            &lineage,
            "nonce-old",
            "nonce-current",
            "session-a"
        )
        .is_err());
    }
}

#[test]
fn attempt_binding_restore_same_session_lineage_refuses_foreign_resume_targets_and_empty_inputs() {
    let foreign_target = [
        entry(Some("session-a"), None, "nonce-old"),
        entry(None, Some("session-b"), "resume-intent"),
    ];
    assert!(prove_same_session_restoration(
        &foreign_target,
        "nonce-old",
        "nonce-current",
        "session-a"
    )
    .is_err());
    assert!(
        prove_same_session_restoration(&foreign_target, "", "nonce-current", "session-a").is_err()
    );
}
