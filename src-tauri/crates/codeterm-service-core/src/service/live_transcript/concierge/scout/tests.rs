use codeterm_decision::{DecisionError, TokenMode};

use super::*;

fn active_tokenless_adapter_input() -> ScoutInput {
    ScoutInput {
        local_concierge: false,
        trigger: TranscriptTrigger::ScoutThenConcierge,
        active_adapter: Some("cli-fixture".into()),
        adapter_registered: true,
        expected_model: Some("cli-fixture/fast-free".into()),
        selected_model: Some("cli-fixture/fast-free".into()),
        supports_choice: true,
        token_mode: TokenMode::None,
        token_usable: false,
        token_acl_invalid: false,
        adapter_backoff: false,
        budget_reached: false,
        credit_exhausted: false,
        storage_ready: true,
    }
}

#[test]
fn active_connected_tokenless_adapter_routes_to_scout_as_ready() {
    let decision = decide_availability(&active_tokenless_adapter_input());
    assert_eq!(decision, ScoutDecision::Ready);
    assert_eq!(decision.status(), ScoutStatus::Ready);
    assert_eq!(decision.reason(), None);
}

#[test]
fn local_concierge_and_non_scout_triggers_disable_scout() {
    let mut local = active_tokenless_adapter_input();
    local.local_concierge = true;
    assert_eq!(decide_availability(&local), ScoutDecision::Disabled);
    assert_eq!(decide_availability(&local).status(), ScoutStatus::Disabled);
    assert_eq!(decide_availability(&local).reason(), None);

    for trigger in [
        TranscriptTrigger::WakeWord,
        TranscriptTrigger::DirectBatch,
        TranscriptTrigger::Manual,
    ] {
        let mut input = active_tokenless_adapter_input();
        input.trigger = trigger;
        assert_eq!(decide_availability(&input), ScoutDecision::Disabled);
    }
}

#[test]
fn every_unavailability_cause_produces_a_specific_reason() {
    let cases: Vec<(ScoutInput, ScoutUnavailableReason, &str)> = vec![
        (
            ScoutInput {
                active_adapter: None,
                ..active_tokenless_adapter_input()
            },
            ScoutUnavailableReason::NotConfigured,
            "configured",
        ),
        (
            ScoutInput {
                adapter_registered: false,
                ..active_tokenless_adapter_input()
            },
            ScoutUnavailableReason::AdapterNotRegistered {
                adapter: "cli-fixture".into(),
            },
            "cli-fixture",
        ),
        (
            ScoutInput {
                selected_model: Some("other/model".into()),
                ..active_tokenless_adapter_input()
            },
            ScoutUnavailableReason::ModelNotApplied {
                adapter: "cli-fixture".into(),
                expected: "cli-fixture/fast-free".into(),
            },
            "fast-free",
        ),
        (
            ScoutInput {
                supports_choice: false,
                ..active_tokenless_adapter_input()
            },
            ScoutUnavailableReason::ChoiceUnsupported {
                adapter: "cli-fixture".into(),
            },
            "choice",
        ),
        (
            ScoutInput {
                token_mode: TokenMode::Required,
                token_usable: false,
                token_acl_invalid: false,
                ..active_tokenless_adapter_input()
            },
            ScoutUnavailableReason::TokenMissing {
                adapter: "cli-fixture".into(),
            },
            "API token",
        ),
        (
            ScoutInput {
                token_mode: TokenMode::Required,
                token_usable: false,
                token_acl_invalid: true,
                ..active_tokenless_adapter_input()
            },
            ScoutUnavailableReason::TokenPermissionsInvalid {
                adapter: "cli-fixture".into(),
            },
            "permissions",
        ),
        (
            ScoutInput {
                adapter_backoff: true,
                ..active_tokenless_adapter_input()
            },
            ScoutUnavailableReason::AdapterBackoff {
                adapter: "cli-fixture".into(),
            },
            "cooling down",
        ),
        (
            ScoutInput {
                credit_exhausted: true,
                ..active_tokenless_adapter_input()
            },
            ScoutUnavailableReason::CreditExhausted {
                adapter: "cli-fixture".into(),
            },
            "credits",
        ),
        (
            ScoutInput {
                budget_reached: true,
                ..active_tokenless_adapter_input()
            },
            ScoutUnavailableReason::BudgetReached,
            "budget",
        ),
        (
            ScoutInput {
                storage_ready: false,
                ..active_tokenless_adapter_input()
            },
            ScoutUnavailableReason::StorageUnavailable,
            "storage",
        ),
    ];

    for (input, expected_reason, expected_fragment) in cases {
        let decision = decide_availability(&input);
        assert_eq!(decision, ScoutDecision::Unavailable(expected_reason));
        assert_eq!(decision.status(), ScoutStatus::Unavailable);
        let reason = decision
            .reason()
            .expect("unavailable decision must explain why");
        assert_ne!(reason, "Scout unavailable");
        assert!(
            reason.contains(expected_fragment),
            "expected {expected_fragment:?} in {reason:?}"
        );
    }
}

#[test]
fn decision_errors_surface_specific_diagnostics_instead_of_generic_text() {
    for (error, fragment) in [
        (DecisionError::Deadline, "timed out"),
        (DecisionError::Budget, "budget"),
        (DecisionError::CreditExhausted, "credits"),
        (DecisionError::Http(402), "credits"),
        (DecisionError::Http(503), "HTTP 503"),
        (DecisionError::Unsupported, "choice"),
        (
            DecisionError::Parse("invalid_answer".into()),
            "invalid_answer",
        ),
        (
            DecisionError::Transport("plugin_decide_failed".into()),
            "plugin_decide_failed",
        ),
        (
            DecisionError::Unavailable("adapter_backoff".into()),
            "cooling down",
        ),
        (
            DecisionError::Unavailable("provider_credentials".into()),
            "credentials",
        ),
        (
            DecisionError::Unavailable("spend_storage_unavailable".into()),
            "spend_storage_unavailable",
        ),
        (
            DecisionError::Unavailable("plugin_unavailable".into()),
            "plugin_unavailable",
        ),
    ] {
        let message = describe_decision_error("cli-fixture", &error);
        assert_ne!(message, "Scout unavailable");
        assert!(
            message.contains(fragment),
            "expected {fragment:?} in {message:?}"
        );
    }
}
