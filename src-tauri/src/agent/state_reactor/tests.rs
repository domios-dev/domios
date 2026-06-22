use super::*;
use crate::agent::pane_state::{
    DelayedResume, PaneState, PaneStateAction, PaneStateClassification, Surface,
};
use crate::agent::status::PaneStatus;

fn classification(
    state: PaneState,
    confidence: f32,
    action: Option<PaneStateAction>,
) -> PaneStateClassification {
    PaneStateClassification {
        state,
        confidence,
        data: None,
        action,
    }
}

fn keys_action(keys: &[&str]) -> PaneStateAction {
    PaneStateAction {
        keys: Some(keys.iter().map(|k| k.to_string()).collect()),
        delayed_resume: None,
        surface: None,
    }
}

#[test]
fn does_not_fire_while_working() {
    // Gate: even a confident, actionable verdict is ignored while streaming.
    let c = classification(
        PaneState::UpdatePrompt,
        0.99,
        Some(keys_action(&["down", "enter"])),
    );
    assert_eq!(
        decide(PaneStatus::Working, Some(&c)),
        ReactorDecision::Ignore
    );
}

#[test]
fn does_not_fire_when_dead() {
    let c = classification(PaneState::UpdatePrompt, 0.99, Some(keys_action(&["enter"])));
    assert_eq!(decide(PaneStatus::Dead, Some(&c)), ReactorDecision::Ignore);
}

#[test]
fn confidence_below_threshold_yields_no_action() {
    let c = classification(
        PaneState::UpdatePrompt,
        GLOBAL_THRESHOLD - 0.01,
        Some(keys_action(&["down", "enter"])),
    );
    assert_eq!(decide(PaneStatus::Idle, Some(&c)), ReactorDecision::Ignore);
}

#[test]
fn no_classification_yields_no_action() {
    assert_eq!(decide(PaneStatus::Idle, None), ReactorDecision::Ignore);
    assert_eq!(decide(PaneStatus::Waiting, None), ReactorDecision::Ignore);
}

#[test]
fn waiting_on_auth_is_never_auto_acted_surface_only() {
    // Even with high confidence AND keys attached, an auth prompt must never
    // press a key — it surfaces only.
    let surface = Surface {
        kind: "auth".to_string(),
        payload: serde_json::json!({ "msg": "paste your token" }),
    };
    let action = PaneStateAction {
        keys: Some(vec!["enter".to_string()]),
        delayed_resume: None,
        surface: Some(surface.clone()),
    };
    let c = classification(PaneState::WaitingOnAuth, 0.99, Some(action));
    assert_eq!(
        decide(PaneStatus::Idle, Some(&c)),
        ReactorDecision::Surface(Some(surface))
    );
}

#[test]
fn waiting_on_auth_with_no_surface_still_surfaces() {
    let action = keys_action(&["enter"]);
    let c = classification(PaneState::WaitingOnAuth, 1.0, Some(action));
    assert_eq!(
        decide(PaneStatus::Waiting, Some(&c)),
        ReactorDecision::Surface(None)
    );
}

#[test]
fn high_confidence_keys_action_is_executed() {
    let action = keys_action(&["down", "enter"]);
    let c = classification(
        PaneState::UpdatePrompt,
        GLOBAL_THRESHOLD,
        Some(action.clone()),
    );
    assert_eq!(
        decide(PaneStatus::Idle, Some(&c)),
        ReactorDecision::Act(action)
    );
}

#[test]
fn high_confidence_action_with_keys_dispatches_key_injection_bytes() {
    // The decided action's keys translate to the same PTY bytes the
    // `send --actions` path injects: arrow-down then carriage return.
    let action = keys_action(&["down", "enter"]);
    let ReactorDecision::Act(decided) = decide(
        PaneStatus::Waiting,
        Some(&classification(PaneState::UpdatePrompt, 0.95, Some(action))),
    ) else {
        panic!("expected Act decision");
    };
    let writes = action_to_writes(&decided);
    assert_eq!(writes, vec![b"\x1b[B".to_vec(), b"\r".to_vec()]);
}

#[test]
fn action_with_no_keys_produces_no_writes() {
    let action = PaneStateAction {
        keys: None,
        delayed_resume: Some(DelayedResume {
            at: "2026-06-19T16:20:00+03:00".to_string(),
            text: "continue your work".to_string(),
            then_keys: None,
        }),
        surface: None,
    };
    assert!(action_to_writes(&action).is_empty());
}

#[test]
fn unknown_keys_are_skipped_not_injected() {
    let action = keys_action(&["down", "frobnicate", "enter"]);
    let writes = action_to_writes(&action);
    assert_eq!(writes, vec![b"\x1b[B".to_vec(), b"\r".to_vec()]);
}

#[test]
fn rfc3339_reset_time_parses_to_epoch_ms() {
    // The +03:00 offset is honored: same instant as 13:20:00 UTC.
    let ms = parse_iso_to_ms("2026-06-19T16:20:00+03:00").expect("parse");
    let expected = chrono::DateTime::parse_from_rfc3339("2026-06-19T13:20:00Z")
        .unwrap()
        .timestamp_millis() as u64;
    assert_eq!(ms, expected);
}

#[test]
fn garbage_reset_time_is_rejected() {
    assert_eq!(parse_iso_to_ms("4:20pm (Europe/Riga)"), None);
}
