use super::*;
use codeterm_types::pty::PromptKind;

fn evidence() -> Evidence<'static> {
    Evidence {
        connected: true,
        agreeing_observations: 2,
        minimum_confidence: 0.90,
        automatic_actions_qualified: true,
        pty_live: true,
        provider_absent: true,
        startup_deadline_elapsed: true,
        prompt_kind: None,
        declared_safe_choice: None,
        model_safe_choice: None,
        safe_choice_confidence: 0.0,
        directory_covered: false,
        automatic_restarts: 0,
        configured_restart_limit: 99,
        restarted_for_label: false,
        owner_required: false,
    }
}

#[test]
fn ga_recovery_table_every_row() {
    let cases = [
        (Label::ReadyIdle, Action::None),
        (Label::WorkingNormally, Action::None),
        (Label::Other, Action::None),
        (Label::UpdatePrompt, Action::Notify),
        (Label::CrashedToShell, Action::None),
        (Label::StillStarting, Action::RestartOnce),
        (Label::ErrorLoop, Action::RestartOnce),
        (Label::LoginRequired, Action::Notify),
        (Label::ModelUnavailable, Action::Notify),
        (Label::TrustOrPermission, Action::Notify),
    ];
    for (label, expected) in cases {
        assert_eq!(decide(label, &evidence()), expected, "{label:?}");
    }
    let mut e = evidence();
    e.prompt_kind = Some(PromptKind::ManualUpdate);
    e.declared_safe_choice = Some("skip-value");
    e.model_safe_choice = Some("skip-value");
    e.safe_choice_confidence = 0.90;
    assert_eq!(
        decide(Label::UpdatePrompt, &e),
        Action::AnswerSafeChoice("skip-value")
    );
    e.prompt_kind = Some(PromptKind::DirectoryTrust);
    e.directory_covered = true;
    assert_eq!(decide(Label::TrustOrPermission, &e), Action::DirectoryTrust);
    e.prompt_kind = Some(PromptKind::Provider);
    assert_eq!(
        decide(Label::TrustOrPermission, &e),
        Action::PermissionPolicy
    );
    e.prompt_kind = Some(PromptKind::Structured);
    assert_eq!(
        decide(Label::TrustOrPermission, &e),
        Action::PermissionPolicy
    );
}

#[test]
fn ga_screen_only_evidence_never_restarts_live_pty() {
    let mut e = evidence();
    e.provider_absent = false;
    for label in [Label::ErrorLoop, Label::StillStarting] {
        assert_eq!(decide(label, &e), Action::Notify);
    }
    e.provider_absent = true;
    assert_eq!(decide(Label::CrashedToShell, &e), Action::None);
}

#[test]
fn ga_max_two_automatic_restarts_then_failure_notification() {
    let mut e = evidence();
    for count in 0..2 {
        e.automatic_restarts = count;
        assert_eq!(decide(Label::ErrorLoop, &e), Action::RestartOnce);
    }
    e.automatic_restarts = 2;
    assert_eq!(decide(Label::ErrorLoop, &e), Action::FailureNotification);
    e.owner_required = true;
    assert_eq!(decide(Label::ErrorLoop, &e), Action::None);
    e.owner_required = false;
    e.automatic_restarts = 0;
    e.restarted_for_label = true;
    assert_eq!(decide(Label::ErrorLoop, &e), Action::Notify);
    assert_eq!(decide(Label::StillStarting, &e), Action::Notify);
}

#[test]
fn ga_thresholds_band_boundaries() {
    let mut e = evidence();
    for (confidence, expected) in [
        (0.59, Action::None),
        (0.60, Action::None),
        (0.79, Action::None),
        (0.80, Action::Notify),
        (0.89, Action::Notify),
        (0.90, Action::RestartOnce),
        (f64::NAN, Action::None),
        (f64::INFINITY, Action::None),
    ] {
        e.minimum_confidence = confidence;
        assert_eq!(decide(Label::ErrorLoop, &e), expected);
    }
    e.minimum_confidence = 0.90;
    e.agreeing_observations = 1;
    assert_eq!(decide(Label::ErrorLoop, &e), Action::None);
    e.agreeing_observations = 2;
    e.connected = false;
    assert_eq!(decide(Label::ErrorLoop, &e), Action::None);
}

#[test]
fn ga_safe_choice_requires_agreement_with_provider_declared_option() {
    let mut e = evidence();
    e.prompt_kind = Some(PromptKind::ManualUpdate);
    e.declared_safe_choice = Some("2");
    e.model_safe_choice = Some("1");
    e.safe_choice_confidence = 1.0;
    assert_eq!(decide(Label::UpdatePrompt, &e), Action::Notify);
    e.model_safe_choice = Some("2");
    e.safe_choice_confidence = 0.89;
    assert_eq!(decide(Label::UpdatePrompt, &e), Action::Notify);
    e.safe_choice_confidence = 0.90;
    assert_eq!(
        decide(Label::UpdatePrompt, &e),
        Action::AnswerSafeChoice("2")
    );
    e.declared_safe_choice = None;
    assert_eq!(decide(Label::UpdatePrompt, &e), Action::Notify);
}

#[test]
fn ga_other_label_never_recovers() {
    assert_eq!(decide(Label::Other, &evidence()), Action::None);
}

#[test]
fn ga_unqualified_heldout_degrades_automatic_rows_to_notify_only() {
    let mut e = evidence();
    e.automatic_actions_qualified = false;
    for label in [Label::ErrorLoop, Label::StillStarting, Label::UpdatePrompt] {
        assert_eq!(decide(label, &e), Action::Notify);
    }
}

#[test]
fn ga_screen_only_crashed_verdict_without_exit_event_does_nothing() {
    assert_eq!(decide(Label::CrashedToShell, &evidence()), Action::None);
}
