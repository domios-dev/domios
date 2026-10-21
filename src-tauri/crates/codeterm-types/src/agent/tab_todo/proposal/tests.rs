use super::*;

#[test]
fn only_parent_or_plan_owner_has_review_authority() {
    assert!(can_review_tab_todo("parent", Some("parent"), None));
    assert!(can_review_tab_todo("owner", Some("parent"), Some("owner")));
    for actor in ["worker", "qa", "stranger", ""] {
        assert!(!can_review_tab_todo(actor, Some("parent"), Some("owner")));
    }
}

#[test]
fn sharing_and_review_are_informational_and_repeatable() {
    let mut proposal = TabTodoProposal::shared(10);
    assert_eq!(proposal.state, TabTodoProposalState::Proposed);
    proposal.review(false, Some("parent".into()), None, 11);
    assert_eq!(proposal.state, TabTodoProposalState::Approved);
    proposal.review(true, Some("parent".into()), Some("Cover errors".into()), 12);
    assert_eq!(proposal.state, TabTodoProposalState::Amended);
    assert_eq!(proposal.shared_at, 10);
    assert_eq!(proposal.reviewed_at, Some(12));
    assert_eq!(proposal.note.as_deref(), Some("Cover errors"));
    assert_eq!(
        TabTodoProposal::shared(13).state,
        TabTodoProposalState::Proposed
    );
}
