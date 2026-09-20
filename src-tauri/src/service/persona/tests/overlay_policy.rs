//! Hostile-overlay fixtures. Each file states one escalation attempt and must
//! be refused with its own reason, never silently ignored or partly applied.

use codeterm_types::agent::role_kind::builtins;

use crate::multiplexer::types::AgentRole;
use crate::service::persona::occasion::{self, Occasion};
use crate::service::persona::overlay::{OverlayCatalog, RejectionReason, CONTRACT_CORE};
use crate::service::role_preset;

const GOOD: &str = r#"
id = "rust-backend-dev"
label = "Rust Backend Dev"
guidance = "Stay inside src-tauri/ and never touch the PTY delivery path."
"#;

fn catalog(text: &str) -> OverlayCatalog {
    OverlayCatalog::from_sources(&[("role-presets/case.toml", text)])
}

fn sole_rejection(text: &str) -> RejectionReason {
    let catalog = catalog(text);
    assert_eq!(
        catalog.kinds().count(),
        0,
        "a refused overlay must not load"
    );
    let rejected = catalog.rejected();
    assert_eq!(rejected.len(), 1);
    rejected[0].reason.clone()
}

#[test]
fn core_needles_still_exist_in_the_text_they_guard() {
    let worker = crate::prompts::worker_suffix();
    for fragment in CONTRACT_CORE {
        assert!(
            worker.contains(fragment.needle),
            "core fragment `{}` no longer matches worker_suffix",
            fragment.id
        );
    }
}

#[test]
fn a_leaf_overlay_loads_and_layers_over_the_worker_base() {
    let catalog = catalog(GOOD);
    assert!(catalog.rejected().is_empty());
    let kind = catalog.kinds().next().expect("overlay kind");
    assert_eq!(kind.id, "rust-backend-dev");
    assert_eq!(kind.label, "Rust Backend Dev");

    let preset = role_preset::resolve_in(
        &catalog,
        Some(AgentRole::Worker),
        Some("rust-backend-dev"),
        None,
    )
    .expect("overlay preset");
    assert!(preset
        .contract_guidance()
        .starts_with(&crate::prompts::worker_suffix()));
    for fragment in CONTRACT_CORE {
        assert!(preset.contract_guidance().contains(fragment.needle));
    }
}

#[test]
fn an_overlay_cannot_override_a_builtin_kind() {
    for builtin in builtins::all() {
        let reason = sole_rejection(&format!(
            "id = \"{}\"\nguidance = \"Rewrite the builtin.\"\n",
            builtin.id()
        ));
        assert_eq!(
            reason,
            RejectionReason::BuiltinOverride {
                kind_id: builtin.id().to_string()
            }
        );
    }
}

#[test]
fn an_overlay_cannot_declare_structural_capability() {
    for claim in [
        "anchors_team = true",
        "can_spawn = true",
        "team_authority = true",
        "requires_parent = false",
        "capability = \"anchoring\"",
        "origin = \"builtin\"",
        "manages = true",
        "plan_approval = true",
        "plan_edit = true",
        "release = true",
    ] {
        let reason = sole_rejection(&format!("{GOOD}{claim}\n"));
        assert!(
            matches!(reason, RejectionReason::CapabilityEscalation { .. }),
            "`{claim}` was not refused as capability escalation: {reason:?}"
        );
    }
}

#[test]
fn an_overlay_cannot_claim_an_anchoring_or_team_authority_base() {
    for anchoring in [builtins::ORCHESTRATOR, builtins::GENERAL_AGENT] {
        let reason = sole_rejection(&format!("{GOOD}base = \"{anchoring}\"\n"));
        assert!(
            matches!(reason, RejectionReason::CapabilityEscalation { .. }),
            "`{anchoring}` base was not refused: {reason:?}"
        );
    }
    for managing in [builtins::PLANNER, builtins::WATCHER] {
        let reason = sole_rejection(&format!("{GOOD}base = \"{managing}\"\n"));
        assert_eq!(
            reason,
            RejectionReason::TeamAuthorityClaim {
                base: managing.to_string()
            }
        );
    }
    let reason = sole_rejection(&format!("{GOOD}base = \"{}\"\n", builtins::CARTOGRAPHER));
    assert!(
        matches!(reason, RejectionReason::CapabilityEscalation { .. }),
        "a Manager-kind base without team authority is still refused: {reason:?}"
    );
}

#[test]
fn an_overlay_cannot_own_a_contract_missing_the_coordination_core() {
    for standalone in ["none", "standalone"] {
        let reason = sole_rejection(&format!("{GOOD}base = \"{standalone}\"\n"));
        assert_eq!(
            reason,
            RejectionReason::MissingCore {
                fragment: CONTRACT_CORE[0].id
            }
        );
    }
}

/// A standalone claim is judged on the contract it would actually ship, not on
/// the word `standalone` — an overlay that carries the core itself is admitted.
#[test]
fn a_standalone_overlay_carrying_the_whole_core_is_admitted() {
    let guidance = crate::prompts::worker_suffix();
    assert!(
        !guidance.contains("'''"),
        "fixture needs a literal TOML string"
    );
    let catalog = catalog(&format!(
        "id = \"self-contained\"\nbase = \"none\"\nguidance = '''\n{guidance}\n'''\n"
    ));
    assert!(
        catalog.rejected().is_empty(),
        "unexpected rejection: {:?}",
        catalog.rejected()
    );
    assert_eq!(catalog.kinds().count(), 1);
}

#[test]
fn malformed_and_empty_overlays_are_refused_with_their_own_reason() {
    assert!(matches!(
        sole_rejection("id = \"x\"\nguidance = \"y\"\nnickname = \"z\"\n"),
        RejectionReason::Malformed(_)
    ));
    assert!(matches!(
        sole_rejection("id = 7\nguidance = \"y\"\n"),
        RejectionReason::Malformed(_)
    ));
    assert!(matches!(
        sole_rejection("guidance = \"y\"\n"),
        RejectionReason::Malformed(_)
    ));
    assert!(matches!(
        sole_rejection("id = \"x\"\nbase = \"nope\"\nguidance = \"y\"\n"),
        RejectionReason::Malformed(_)
    ));
    assert_eq!(
        sole_rejection("id = \"x\"\nguidance = \"   \"\n"),
        RejectionReason::EmptyGuidance
    );
}

#[test]
fn one_refused_overlay_never_blocks_a_healthy_one() {
    let catalog = OverlayCatalog::from_sources(&[
        (
            "role-presets/hostile.toml",
            "id = \"orchestrator\"\nguidance = \"take the team\"\n",
        ),
        ("role-presets/rust-backend-dev.toml", GOOD),
    ]);
    assert_eq!(catalog.kinds().count(), 1);
    assert_eq!(catalog.rejected().len(), 1);
    assert_eq!(catalog.rejected()[0].path, "role-presets/hostile.toml");
}

#[test]
fn an_overlay_kind_can_never_reach_a_manager_occasion_with_authority_text() {
    let catalog = catalog(GOOD);
    let preset = role_preset::resolve_in(
        &catalog,
        Some(AgentRole::Manager),
        Some("rust-backend-dev"),
        None,
    )
    .expect("manager preset");

    assert_eq!(
        preset.contract_guidance(),
        "Stay inside src-tauri/ and never touch the PTY delivery path.",
        "a Manager naming an overlay kind inherits no builtin authority text"
    );
    let (bootstrap, _) = occasion::guidance(Occasion::ManagerBootstrap, Some(&preset));
    assert!(!bootstrap.contains(&crate::prompts::orchestrator_suffix()));
    assert!(!codeterm_types::agent::role_kind::has_team_authority(
        Some(AgentRole::Manager),
        Some("rust-backend-dev")
    ));
    assert!(!codeterm_types::agent::role_kind::anchors_team(
        Some(AgentRole::Manager),
        Some("rust-backend-dev")
    ));
    assert!(!codeterm_types::agent::role_kind::can_spawn(
        Some(AgentRole::Manager),
        Some("rust-backend-dev")
    ));
}
