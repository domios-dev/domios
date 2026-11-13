use std::collections::{BTreeMap, BTreeSet};

use codeterm_plan::service_plan::state::mission_control::ProviderAllowance;
use codeterm_team_package::contracts::{
    ModelBlockedReason, ModelDecision, ModelDecisionReason, ModelOption,
};
use codeterm_team_package::{ModelPreference, RoleKind, TeamDocument};
use serde_json::json;

use super::target::allowance_exhausted;
use super::*;
use crate::agent::provider::{ModelInfo, ReasoningEffortInfo};
use crate::service::spawn::spawn_model_catalogue::close_model_ids;

fn model(id: &str, efforts: Option<&[&str]>) -> ModelInfo {
    ModelInfo {
        id: id.into(),
        display_name: id.into(),
        reasoning_efforts: efforts.map(|levels| {
            levels
                .iter()
                .map(|level| ReasoningEffortInfo {
                    id: (*level).into(),
                    display_name: (*level).into(),
                    ..Default::default()
                })
                .collect()
        }),
        ..Default::default()
    }
}

fn harness(models: Vec<ModelInfo>, efforts: &[&str]) -> HarnessCatalogue {
    HarnessCatalogue {
        readiness: HarnessReadiness::Ready,
        models,
        enforced: true,
        efforts: efforts.iter().map(|level| (*level).into()).collect(),
        default_effort: None,
        default_model: None,
    }
}

const CLAUDE_SCALE: &[&str] = &["low", "medium", "high", "xhigh", "max"];

fn target() -> TargetCatalogue {
    let mut harnesses = BTreeMap::new();
    harnesses.insert(
        "claude".to_string(),
        harness(
            vec![
                model("opus", Some(CLAUDE_SCALE)),
                model("sonnet", Some(&["low", "medium", "high"])),
                model("haiku", Some(&[])),
            ],
            CLAUDE_SCALE,
        ),
    );
    harnesses.insert(
        "codex".to_string(),
        harness(
            vec![
                model("gpt-6-sol", None),
                model("gpt-6.5-sol", None),
                model("gpt-6-astra", None),
            ],
            &["low", "medium", "high", "xhigh"],
        ),
    );
    TargetCatalogue {
        harnesses,
        tools: None,
    }
}

fn with_readiness(harness: &str, readiness: HarnessReadiness) -> TargetCatalogue {
    let mut target = target();
    target.harnesses.get_mut(harness).unwrap().readiness = readiness;
    target
}

fn list(models: &[&str]) -> ModelPreference {
    ModelPreference::List(models.iter().map(|model| (*model).into()).collect())
}

fn request(kind: RoleKind, preference: Option<&ModelPreference>) -> RoleModelRequest<'_> {
    RoleModelRequest {
        kind,
        preference,
        effort: None,
        owner_choices: &[],
        requires_harnesses: &[],
        tools: &[],
        lost: &[],
    }
}

fn manager(preference: &ModelPreference) -> RoleModelRequest<'_> {
    request(RoleKind::Manager, Some(preference))
}

fn models(options: &[ModelOption]) -> Vec<&str> {
    options.iter().map(|option| option.model.as_str()).collect()
}

fn choice(decision: &ModelDecision) -> Option<&str> {
    decision.choice.as_ref().map(|choice| choice.model.as_str())
}

#[test]
fn missing_manager_model_needs_owner_choice() {
    let decision = decide_model(&request(RoleKind::Manager, None), None, &target());
    assert_eq!(decision.reason, ModelDecisionReason::NeedsChoice);
    assert_eq!(choice(&decision), None);
    assert_eq!(decision.blocked, None);
    assert_eq!(
        models(&decision.alternatives),
        [
            "claude:opus",
            "claude:sonnet",
            "claude:haiku",
            "codex:gpt-6-sol",
            "codex:gpt-6.5-sol",
            "codex:gpt-6-astra",
        ]
    );
    assert!(decision
        .alternatives
        .iter()
        .all(|option| option.price.is_none()));
}

#[test]
fn model_less_worker_takes_normal_dispatch_defaults() {
    let decision = decide_model(&request(RoleKind::Worker, None), None, &target());
    assert_eq!(decision.reason, ModelDecisionReason::NormalDefault);
    assert_eq!(choice(&decision), None);
    assert!(decision.alternatives.is_empty());
    assert!(!decision.amber);
}

#[test]
fn first_preference_is_chosen_with_in_list_alternatives() {
    let preference = list(&["claude:opus", "codex:gpt-6-sol"]);
    let decision = decide_model(&manager(&preference), None, &target());
    assert_eq!(decision.reason, ModelDecisionReason::FirstPreference);
    assert_eq!(choice(&decision), Some("claude:opus"));
    assert_eq!(models(&decision.alternatives), ["codex:gpt-6-sol"]);
    assert!(!decision.amber);
    assert_eq!(decision.choice.as_ref().unwrap().price, None);
}

#[test]
fn within_list_fallback_is_amber() {
    let preference = list(&["claude:opus", "codex:gpt-6-sol"]);
    let target = with_readiness("claude", HarnessReadiness::Quota);
    let decision = decide_model(&manager(&preference), None, &target);
    assert_eq!(decision.reason, ModelDecisionReason::WithinListFallback);
    assert_eq!(choice(&decision), Some("codex:gpt-6-sol"));
    assert!(decision.amber);
}

#[test]
fn quota_blocks_and_offers_outside_list_choices_for_a_human_pick() {
    let preference = list(&["claude:opus"]);
    let target = with_readiness("claude", HarnessReadiness::Quota);
    let decision = decide_model(&manager(&preference), None, &target);
    assert_eq!(decision.reason, ModelDecisionReason::Blocked);
    assert_eq!(
        decision.blocked,
        Some(ModelBlockedReason::Quota {
            harness: "claude".into()
        })
    );
    assert_eq!(choice(&decision), None);
    assert_eq!(
        models(&decision.alternatives),
        ["codex:gpt-6-sol", "codex:gpt-6.5-sol", "codex:gpt-6-astra"]
    );
}

#[test]
fn outside_list_candidate_is_never_chosen_automatically() {
    let preference = list(&["claude:opus", "claude:sonnet"]);
    for readiness in [
        HarnessReadiness::Absent,
        HarnessReadiness::NotSignedIn,
        HarnessReadiness::Quota,
    ] {
        let decision = decide_model(
            &manager(&preference),
            None,
            &with_readiness("claude", readiness),
        );
        assert_eq!(choice(&decision), None, "{readiness:?}");
        assert!(!decision.alternatives.is_empty());
    }
    let owner = ["codex:gpt-6-astra".to_string()];
    let picked = RoleModelRequest {
        owner_choices: &owner,
        ..manager(&preference)
    };
    let decision = decide_model(&picked, None, &target());
    assert_eq!(decision.reason, ModelDecisionReason::OwnerChoice);
    assert_eq!(choice(&decision), Some("codex:gpt-6-astra"));
}

#[test]
fn same_as_takes_the_referenced_roles_resolved_model() {
    let document: TeamDocument = serde_json::from_value(json!({
        "name": "dev.example/decide",
        "version": "1.0.0",
        "description": "Decision fixture.",
        "roles": {
            "orchestrator": {"kind": "manager", "model": ["gemini:pro", "codex:gpt-6-sol"], "reasoning": "high"},
            "planner": {"kind": "manager", "model": {"same_as": "orchestrator"}},
            "reviewer": {"kind": "worker"}
        }
    }))
    .unwrap();
    let before = document.clone();
    let decided = decide_team_models(&document, &TeamModelInputs::default(), &target());
    assert_eq!(document, before);
    let orchestrator = &decided["orchestrator"];
    assert_eq!(orchestrator.reason, ModelDecisionReason::WithinListFallback);
    assert_eq!(choice(orchestrator), Some("codex:gpt-6-sol"));
    assert_eq!(orchestrator.effort.as_ref().unwrap().applied, "high");
    let planner = &decided["planner"];
    assert_eq!(planner.reason, ModelDecisionReason::SameAs);
    assert_eq!(choice(planner), Some("codex:gpt-6-sol"));
    assert_eq!(
        decided["reviewer"].reason,
        ModelDecisionReason::NormalDefault
    );
}

#[test]
fn same_as_is_blocked_when_the_referenced_model_is_absent_on_the_target() {
    let referenced = ModelDecision {
        choice: Some(ModelOption {
            model: "gemini:pro".into(),
            price: None,
        }),
        reason: ModelDecisionReason::FirstPreference,
        alternatives: Vec::new(),
        blocked: None,
        amber: false,
        effort: None,
    };
    let preference = ModelPreference::SameAs {
        same_as: "orchestrator".into(),
    };
    let decision = decide_model(&manager(&preference), Some(&referenced), &target());
    assert_eq!(decision.reason, ModelDecisionReason::Blocked);
    assert_eq!(
        decision.blocked,
        Some(ModelBlockedReason::SameAsUnavailable {
            role: "orchestrator".into()
        })
    );
}

#[test]
fn renamed_id_returns_ranked_close_id_suggestions() {
    let preference = list(&["codex:gpt-6.1-sol"]);
    let decision = decide_model(&manager(&preference), None, &target());
    assert_eq!(decision.reason, ModelDecisionReason::Blocked);
    assert_eq!(
        decision.blocked,
        Some(ModelBlockedReason::UnknownModel {
            model: "codex:gpt-6.1-sol".into(),
            suggestions: vec![
                "codex:gpt-6.5-sol".into(),
                "codex:gpt-6-sol".into(),
                "codex:gpt-6-astra".into(),
            ],
        })
    );
    assert_eq!(decision.alternatives[0].model, "codex:gpt-6-sol");
}

#[test]
fn close_id_ties_keep_catalogue_order() {
    let ids = |values: &[&str]| {
        values
            .iter()
            .map(|id| (*id).to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        close_model_ids("gpt-6.1-sol", &ids(&["gpt-6.5-sol", "gpt-6.2-sol"])),
        ["gpt-6.5-sol", "gpt-6.2-sol"]
    );
    assert_eq!(
        close_model_ids("gpt-6.1-sol", &ids(&["gpt-6.2-sol", "gpt-6.5-sol"])),
        ["gpt-6.2-sol", "gpt-6.5-sol"]
    );
}

#[test]
fn unsupported_effort_clamps_to_the_nearest_level_with_a_tooltip_reason() {
    let preference = list(&["claude:sonnet"]);
    let decide_with = |effort: &str| {
        let request = RoleModelRequest {
            effort: Some(effort),
            ..manager(&preference)
        };
        decide_model(&request, None, &target()).effort.unwrap()
    };
    let clamped = decide_with("xhigh");
    assert_eq!(clamped.requested, "xhigh");
    assert_eq!(clamped.applied, "high");
    assert!(clamped.reason.is_some());
    assert_eq!(decide_with("max").applied, "high");
    assert_eq!(decide_with("minimal").applied, "low");
    let kept = decide_with("medium");
    assert_eq!(kept.applied, "medium");
    assert_eq!(kept.reason, None);
}

#[test]
fn model_without_reasoning_effort_fails_the_effort_hard_need() {
    let preference = list(&["claude:haiku", "claude:sonnet"]);
    let request = RoleModelRequest {
        effort: Some("high"),
        ..manager(&preference)
    };
    let decision = decide_model(&request, None, &target());
    assert_eq!(choice(&decision), Some("claude:sonnet"));
    assert!(decision.amber);
    let only = list(&["claude:haiku"]);
    let request = RoleModelRequest {
        effort: Some("high"),
        ..manager(&only)
    };
    assert_eq!(
        decide_model(&request, None, &target()).blocked,
        Some(ModelBlockedReason::NoReasoningEffort {
            model: "claude:haiku".into()
        })
    );
}

#[test]
fn absent_harness_is_blocked_with_recovery_choices() {
    let preference = list(&["gemini:pro"]);
    let decision = decide_model(&manager(&preference), None, &target());
    assert_eq!(
        decision.blocked,
        Some(ModelBlockedReason::AbsentHarness {
            harness: "gemini".into()
        })
    );
    assert!(!decision.alternatives.is_empty());
    let required = ["gemini".to_string()];
    let opus = list(&["claude:opus"]);
    let request = RoleModelRequest {
        requires_harnesses: &required,
        ..manager(&opus)
    };
    assert_eq!(
        decide_model(&request, None, &target()).blocked,
        Some(ModelBlockedReason::AbsentHarness {
            harness: "gemini".into()
        })
    );
}

#[test]
fn unsigned_in_acp_harness_is_blocked() {
    let mut target = target();
    let mut acp = harness(vec![model("pro", None)], &[]);
    acp.readiness = HarnessReadiness::NotSignedIn;
    acp.enforced = false;
    target.harnesses.insert("acp".into(), acp);
    let preference = list(&["acp:pro"]);
    let decision = decide_model(&manager(&preference), None, &target);
    assert_eq!(decision.reason, ModelDecisionReason::Blocked);
    assert_eq!(
        decision.blocked,
        Some(ModelBlockedReason::NotSignedIn {
            harness: "acp".into()
        })
    );
}

#[test]
fn missing_tool_blocks_only_when_the_target_reports_its_inventory() {
    let preference = list(&["claude:opus"]);
    let tools = ["mcp:github".to_string()];
    let request = RoleModelRequest {
        tools: &tools,
        ..manager(&preference)
    };
    assert_eq!(
        choice(&decide_model(&request, None, &target())),
        Some("claude:opus")
    );
    let mut reported = target();
    reported.tools = Some(BTreeSet::new());
    assert_eq!(
        decide_model(&request, None, &reported).blocked,
        Some(ModelBlockedReason::MissingTool {
            tool: "mcp:github".into()
        })
    );
}

#[test]
fn mid_run_loss_takes_the_next_preference_then_asks_the_owner() {
    let preference = list(&["claude:opus", "codex:gpt-6-sol"]);
    let lost = ["claude:opus".to_string()];
    let request = RoleModelRequest {
        lost: &lost,
        ..manager(&preference)
    };
    let next = decide_model(&request, None, &target());
    assert_eq!(next.reason, ModelDecisionReason::WithinListFallback);
    assert_eq!(choice(&next), Some("codex:gpt-6-sol"));
    assert!(next.amber);

    let lost = ["claude:opus".to_string(), "codex:gpt-6-sol".to_string()];
    let request = RoleModelRequest {
        lost: &lost,
        ..manager(&preference)
    };
    let exhausted = decide_model(&request, None, &target());
    assert_eq!(exhausted.reason, ModelDecisionReason::NeedsChoice);
    assert_eq!(
        exhausted.blocked,
        Some(ModelBlockedReason::PreferencesExhausted)
    );
    assert_eq!(choice(&exhausted), None);
    assert!(!models(&exhausted.alternatives)
        .iter()
        .any(|model| lost.iter().any(|lost| lost.as_str() == *model)));
}

#[test]
fn target_peer_catalogue_decides_from_its_provider_listing() {
    let peer = TargetCatalogue::from_provider_listing(&[
        json!({
            "id": "claude",
            "cli_status": {"installed": false},
            "available_models": [{"id": "opus", "display_name": "Opus"}]
        }),
        json!({
            "id": "codex",
            "cli_status": {"installed": true},
            "model_catalogue": {"authority": "authoritative"},
            "available_models": [
                {
                    "id": "gpt-6-sol",
                    "display_name": "GPT-6 Sol",
                    "reasoning_efforts": [
                        {"id": "low", "display_name": "Low"},
                        {"id": "high", "display_name": "High"}
                    ]
                },
                {"id": "gpt-6-astra", "display_name": "GPT-6 Astra"}
            ],
            "reasoning_efforts": [
                {"id": "low", "display_name": "Low"},
                {"id": "medium", "display_name": "Medium"},
                {"id": "high", "display_name": "High"},
                {"id": "xhigh", "display_name": "Extra high"}
            ],
            "default_model": "gpt-6-astra"
        }),
    ]);
    let preference = list(&["claude:opus", "codex:gpt-6-sol"]);
    let request = RoleModelRequest {
        effort: Some("xhigh"),
        ..manager(&preference)
    };
    let decision = decide_model(&request, None, &peer);
    assert_eq!(decision.reason, ModelDecisionReason::WithinListFallback);
    assert_eq!(choice(&decision), Some("codex:gpt-6-sol"));
    assert_eq!(decision.effort.unwrap().applied, "high");

    let renamed = list(&["codex:gpt-6.1-sol"]);
    let decision = decide_model(&manager(&renamed), None, &peer);
    assert!(matches!(
        decision.blocked,
        Some(ModelBlockedReason::UnknownModel { ref suggestions, .. })
            if suggestions[0] == "codex:gpt-6-sol"
    ));
    assert_eq!(decision.alternatives[0].model, "codex:gpt-6-astra");
}

#[test]
fn exhausted_allowance_is_quota_until_its_window_resets() {
    let allowance = |used: f32, resets_at: Option<i64>, stale: Option<&str>| ProviderAllowance {
        captured_at: 0,
        session_used_pct: Some(used),
        session_resets_at: resets_at,
        weekly_used_pct: None,
        weekly_resets_at: None,
        stale_reason: stale.map(str::to_string),
    };
    assert!(allowance_exhausted(
        &allowance(100.0, Some(2_000), None),
        1_000
    ));
    assert!(!allowance_exhausted(
        &allowance(100.0, Some(500), None),
        1_000
    ));
    assert!(!allowance_exhausted(&allowance(99.0, None, None), 1_000));
    assert!(!allowance_exhausted(
        &allowance(100.0, None, Some("parse_failed")),
        1_000
    ));
}
