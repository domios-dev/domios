//! The inspection surface must render the resolver's own text, never a
//! paraphrase, and must name where each fragment came from.

use codeterm_team_package::{RoleKind, RoleStart, StartMode};
use codeterm_types::agent::role_kind::{builtins, RoleType};
use codeterm_types::agent::AgentRole;
use codeterm_types::team::role_registration::{PinnedPackage, RoleProvenance, RoleRegistration};

use crate::persona::inspect::{self, PROBE_PAYLOAD};
use crate::persona::occasion::{self, Occasion};
use crate::role_preset;

fn team_worker(name: &str) -> RoleRegistration {
    RoleRegistration {
        team_id: "team_fixture".into(),
        kind: RoleKind::Worker,
        name: name.into(),
        label: name.into(),
        description: Some("fixture-description".into()),
        start: RoleStart::Mode(StartMode::OnDemand),
        package: PinnedPackage {
            name: "com.example/fixture".into(),
            version: "3.1.0".into(),
        },
        instructions: "fixture-team-instructions".into(),
        model: None,
        reasoning: None,
        consent_decision_id: None,
        files: Vec::new(),
        skills: Vec::new(),
        registered_at: 1,
    }
}

#[test]
fn show_renders_the_exact_resolver_text_for_every_occasion() {
    let role_type = RoleType::new(AgentRole::Manager, builtins::ORCHESTRATOR);
    let view = inspect::show(&[], &role_type, None, None).expect("orchestrator view");

    assert_eq!(view.role_type, builtins::ORCHESTRATOR);
    assert_eq!(view.label, "Orchestrator");
    assert_eq!(view.role, "Manager");
    assert!(matches!(
        view.provenance,
        Some(RoleProvenance::Builtin { .. })
    ));

    let preset =
        role_preset::resolve(Some(AgentRole::Manager), Some(builtins::ORCHESTRATOR), None).unwrap();
    for rendered in &view.occasions {
        let occasion = Occasion::parse(rendered.occasion).expect("known occasion");
        let (text, emission) = occasion::guidance(occasion, Some(&preset));
        assert_eq!(rendered.text, text, "{} text drifted", rendered.occasion);
        assert_eq!(rendered.emission, emission.label());
        assert_eq!(
            rendered.composed,
            occasion::compose(occasion, Some(&preset), Some(PROBE_PAYLOAD))
        );
    }
}

#[test]
fn show_names_the_team_package_of_a_registered_role() {
    let team = [team_worker("checker")];
    let role_type = RoleType::new(AgentRole::Worker, "checker");
    let view = inspect::show(&team, &role_type, Some("fixture-persona"), None).unwrap();
    assert_eq!(
        view.provenance,
        Some(RoleProvenance::Team {
            package: team[0].package.clone()
        })
    );
    assert_eq!(view.fragments[0].source, "team_registration");
    assert_eq!(view.fragments[0].package.as_ref(), Some(&team[0].package));
    assert_eq!(view.fragments.last().unwrap().source, "task_overlay");
}

#[test]
fn list_shows_exactly_the_team_roles_with_kind_authority_and_provenance() {
    let mut custom = team_worker("security_lead");
    custom.kind = RoleKind::Manager;
    let team = [team_worker("checker"), team_worker(builtins::QA), custom];
    let view = inspect::list(Some("team_fixture"), &team);
    let names: Vec<&str> = view
        .roles
        .iter()
        .map(|role| role.role_type.as_str())
        .collect();
    assert_eq!(
        names,
        ["checker", builtins::QA, "security_lead", builtins::WATCHER]
    );
    let find = |name: &str| {
        view.roles
            .iter()
            .find(|role| role.role_type == name)
            .unwrap_or_else(|| panic!("{name} listed"))
    };
    assert!(matches!(
        find("checker").provenance,
        RoleProvenance::Team { .. }
    ));
    let checker = find("checker");
    assert_eq!(
        (checker.kind, checker.authority),
        (RoleKind::Worker, "none")
    );
    assert!(!checker.can_spawn && !checker.team_authority && !checker.anchors_team);
    let security = find("security_lead");
    assert_eq!(
        (security.kind, security.authority),
        (RoleKind::Manager, "none")
    );
    assert!(!security.can_spawn && !security.team_authority && !security.anchors_team);
    assert!(find(builtins::WATCHER).host_only);
}

#[test]
fn a_team_without_registrations_lists_the_default_package() {
    let view = inspect::list(None, &[]);
    let orchestrator = view
        .roles
        .iter()
        .find(|role| role.role_type == builtins::ORCHESTRATOR)
        .expect("orchestrator listed");
    assert!(matches!(
        orchestrator.provenance,
        RoleProvenance::Builtin { .. }
    ));
    assert_eq!(orchestrator.authority, "Orchestrator");
    assert!(orchestrator.anchors_team);
    assert_eq!(view.roles.len(), 9, "eight package roles and the watcher");
}
