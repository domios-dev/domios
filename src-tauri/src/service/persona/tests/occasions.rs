//! Properties the composition table must hold for every occasion.

use codeterm_types::agent::role_kind::builtins;

use crate::multiplexer::types::AgentRole;
use crate::service::persona::occasion::{
    compose, declared_fallback, guidance, Emission, Occasion, ALL,
};
use crate::service::persona::overlay::OverlayCatalog;
use crate::service::role_preset;

const UNMATCHED_KIND: &str = "field-marshal";

fn manager_preset(kind: &str) -> role_preset::RolePreset {
    role_preset::resolve(Some(AgentRole::Manager), Some(kind), None).expect("manager preset")
}

fn worker_preset(kind: Option<&str>, persona: Option<&str>) -> role_preset::RolePreset {
    role_preset::resolve(Some(AgentRole::Worker), kind, persona).expect("worker preset")
}

#[test]
fn every_occasion_declares_a_fallback_that_is_never_empty() {
    for occasion in ALL {
        let preset = manager_preset(UNMATCHED_KIND);
        let (text, emission) = guidance(*occasion, Some(&preset));
        match declared_fallback(*occasion) {
            Some(declared) => {
                assert!(
                    !declared.trim().is_empty(),
                    "{} declares an empty fallback",
                    occasion.id()
                );
                assert_eq!(text, declared, "{} ignored its fallback", occasion.id());
                assert_eq!(emission, Emission::Fallback);
            }
            None => {
                // The payload itself is the declared fallback for this occasion.
                assert!(text.is_empty());
                assert_eq!(
                    compose(*occasion, Some(&preset), Some("payload")),
                    Some("payload".to_string()),
                    "{} must still emit its payload",
                    occasion.id()
                );
            }
        }
    }
}

#[test]
fn an_unmatched_pair_never_composes_to_nothing_when_a_payload_exists() {
    for occasion in ALL {
        let preset = manager_preset(UNMATCHED_KIND);
        let composed = compose(*occasion, Some(&preset), Some("payload"))
            .unwrap_or_else(|| panic!("{} dropped its payload", occasion.id()));
        assert!(composed.contains("payload"));
        assert!(!composed.is_empty());
    }
}

#[test]
fn occasion_ids_round_trip_and_are_unique() {
    let mut ids: Vec<&str> = ALL.iter().map(|o| o.id()).collect();
    let count = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count, "occasion ids must be unique");
    for occasion in ALL {
        assert_eq!(Occasion::parse(occasion.id()), Some(*occasion));
    }
    assert_eq!(Occasion::parse("no_such_occasion"), None);
}

#[test]
fn every_builtin_manager_kind_reaches_the_same_resolver() {
    for descriptor in builtins::all() {
        if matches!(descriptor.id(), builtins::WORKER | builtins::CARTOGRAPHER) {
            continue;
        }
        let preset = manager_preset(descriptor.id());
        assert_eq!(preset.kind_id, descriptor.id());
        assert_eq!(preset.label, descriptor.label());
        let (text, _) = guidance(Occasion::ManagerBootstrap, Some(&preset));
        assert!(
            !text.trim().is_empty(),
            "{} bootstraps with no guidance",
            descriptor.id()
        );
    }
}

#[test]
fn the_worker_identity_slot_carries_the_persona_and_the_assignment_slot_carries_the_contract() {
    let preset = worker_preset(Some("rust-backend-dev"), Some("You are Mira."));

    let (identity, identity_emission) = guidance(Occasion::WorkerLaunchIdentity, Some(&preset));
    assert_eq!(identity, "You are Mira.");
    assert_eq!(identity_emission, Emission::Preset);

    let (assignment, assignment_emission) = guidance(Occasion::LaunchAssignment, Some(&preset));
    assert_eq!(assignment, crate::prompts::worker_suffix());
    assert_eq!(assignment_emission, Emission::Preset);
    assert!(
        !assignment.contains("You are Mira."),
        "the delivery persona must not be duplicated into the assignment slot"
    );
}

#[test]
fn a_disk_overlay_layers_into_the_contract_slot_only() {
    let catalog = OverlayCatalog::from_sources(&[(
        "role-presets/rust-backend-dev.toml",
        r#"
id = "rust-backend-dev"
label = "Rust Backend Dev"
guidance = "Stay inside src-tauri/."
"#,
    )]);
    let preset = role_preset::resolve_in(
        &catalog,
        Some(AgentRole::Worker),
        Some("rust-backend-dev"),
        Some("You are Mira."),
    )
    .expect("overlay preset");

    let (identity, _) = guidance(Occasion::WorkerLaunchIdentity, Some(&preset));
    assert_eq!(identity, "You are Mira.");

    let (assignment, _) = guidance(Occasion::LaunchAssignment, Some(&preset));
    assert_eq!(
        assignment,
        format!(
            "{}\n\nStay inside src-tauri/.",
            crate::prompts::worker_suffix()
        )
    );
}

#[test]
fn a_silent_pair_contributes_nothing_and_still_carries_its_payload() {
    for kind in [builtins::WATCHER, builtins::GENERAL_AGENT] {
        let preset = manager_preset(kind);
        let (text, emission) = guidance(Occasion::LaunchAssignment, Some(&preset));
        assert!(text.is_empty(), "{kind} must contribute no assignment text");
        assert_eq!(emission, Emission::Silent);
        assert_eq!(
            compose(Occasion::LaunchAssignment, Some(&preset), Some("payload")),
            Some("payload".to_string())
        );
        assert_eq!(
            compose(Occasion::LaunchAssignment, Some(&preset), None),
            None
        );
    }
}

#[test]
fn only_an_anchor_states_its_charter_without_an_assignment() {
    let orchestrator = manager_preset(builtins::ORCHESTRATOR);
    assert_eq!(
        compose(Occasion::LaunchAssignment, Some(&orchestrator), None),
        Some(crate::prompts::orchestrator_suffix())
    );

    for kind in [builtins::PLANNER, builtins::WATCHER] {
        assert_eq!(
            compose(
                Occasion::LaunchAssignment,
                Some(&manager_preset(kind)),
                None
            ),
            None,
            "{kind} must not broadcast a charter with no assignment"
        );
    }
    assert_eq!(
        compose(
            Occasion::LaunchAssignment,
            Some(&worker_preset(None, None)),
            None
        ),
        None
    );
}
