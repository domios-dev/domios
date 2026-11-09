use super::*;

fn row(kind: &str) -> ChatMsg {
    ChatMsg {
        id: format!("opaque-{kind}"),
        kind: kind.into(),
        content: "opaque".into(),
        ..Default::default()
    }
}

#[test]
fn only_a_readable_transcript_without_agent_work_starts_fresh() {
    assert_eq!(choose(Some(&[])), LostSessionChoice::FreshLaunch);
    assert_eq!(
        choose(Some(&[row("user"), row("system"), row("status")])),
        LostSessionChoice::FreshLaunch
    );
    for kind in ["assistant", "thinking", "tool", "tool_call", "tool_result"] {
        assert_eq!(
            choose(Some(&[row("user"), row(kind)])),
            LostSessionChoice::Owner,
            "{kind}"
        );
    }
    assert_eq!(choose(None), LostSessionChoice::Owner);
}

#[test]
fn rows_inherited_from_a_fork_parent_are_not_this_sessions_work() {
    let mut inherited = row("assistant");
    inherited.inherited_from_session_id = Some("opaque-parent".into());
    assert_eq!(
        choose(Some(&[inherited, row("user")])),
        LostSessionChoice::FreshLaunch
    );
}

#[test]
fn each_lost_session_is_claimed_once_within_a_bounded_registry() {
    let tab = uuid::Uuid::new_v4().to_string();
    assert!(claim(&tab, "opaque-thread"));
    assert!(!claim(&tab, "opaque-thread"));
    assert!(claim(&tab, "opaque-other-thread"));
    for number in 0..ATTEMPT_CAP * 2 {
        claim(&tab, &format!("filler-{number}"));
    }
    assert!(ATTEMPTED.lock().len() <= ATTEMPT_CAP);
}
