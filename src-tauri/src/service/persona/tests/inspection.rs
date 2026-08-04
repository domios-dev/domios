//! The inspection surface must render the resolver's own text, never a
//! paraphrase, and must name where each fragment came from.

use codeterm_types::agent::role_kind::builtins;

use crate::multiplexer::types::AgentRole;
use crate::service::persona::inspect::{self, PROBE_PAYLOAD};
use crate::service::persona::occasion::{self, Occasion};
use crate::service::persona::overlay::OverlayCatalog;
use crate::service::role_preset;

const OVERLAY: &str = r#"
id = "rust-backend-dev"
label = "Rust Backend Dev"
guidance = "Stay inside src-tauri/."
"#;

#[test]
fn show_renders_the_exact_resolver_text_for_every_occasion() {
    let catalog = OverlayCatalog::empty();
    let view = inspect::show(
        &catalog,
        AgentRole::Manager,
        Some(builtins::ORCHESTRATOR),
        None,
        None,
    )
    .expect("orchestrator view");

    assert_eq!(view.kind_id, builtins::ORCHESTRATOR);
    assert_eq!(view.label, "Orchestrator");
    assert_eq!(view.origin, "builtin");
    assert_eq!(view.role, "Manager");

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
    let launch = view
        .occasions
        .iter()
        .find(|o| o.occasion == Occasion::LaunchAssignment.id())
        .expect("launch occasion");
    assert_eq!(launch.text, crate::prompts::orchestrator_suffix());
    assert_eq!(launch.emission, "preset");
}

#[test]
fn show_reports_the_declared_fallback_when_the_preset_carries_nothing() {
    let view = inspect::show(
        &OverlayCatalog::empty(),
        AgentRole::Manager,
        Some("field-marshal"),
        None,
        None,
    )
    .expect("unmatched manager view");

    assert_eq!(view.origin, "unregistered");
    let bootstrap = view
        .occasions
        .iter()
        .find(|o| o.occasion == Occasion::ManagerBootstrap.id())
        .expect("bootstrap occasion");
    assert_eq!(bootstrap.emission, "declared_fallback");
    assert_eq!(bootstrap.text, crate::prompts::planner_suffix());
}

#[test]
fn show_names_the_provenance_of_every_fragment() {
    let catalog = OverlayCatalog::from_sources(&[("role-presets/rust-backend-dev.toml", OVERLAY)]);
    let view = inspect::show(
        &catalog,
        AgentRole::Worker,
        Some("rust-backend-dev"),
        Some("You are Mira."),
        None,
    )
    .expect("overlay view");

    assert_eq!(view.origin, "overlay");
    assert_eq!(view.label, "Rust Backend Dev");
    assert_eq!(
        view.fragments
            .iter()
            .map(|fragment| fragment.source)
            .collect::<Vec<_>>(),
        vec!["inherited_base", "disk_overlay", "task_overlay"]
    );
    assert_eq!(
        view.fragments[1].path.as_deref(),
        Some("role-presets/rust-backend-dev.toml"),
        "a disk fragment names the file it came from"
    );
    assert!(view.fragments[0].path.is_none());
    assert_eq!(view.fragments[2].text, "You are Mira.");
}

#[test]
fn list_covers_every_builtin_kind_and_reports_refused_overlays() {
    let catalog = OverlayCatalog::from_sources(&[
        ("role-presets/rust-backend-dev.toml", OVERLAY),
        (
            "role-presets/hostile.toml",
            "id = \"claimant\"\nguidance = \"g\"\nteam_authority = true\n",
        ),
    ]);
    let view = inspect::list(&catalog);

    for descriptor in builtins::all() {
        let listed = view
            .kinds
            .iter()
            .find(|kind| kind.kind_id == descriptor.id())
            .unwrap_or_else(|| panic!("{} missing from listing", descriptor.id()));
        assert_eq!(listed.origin, "builtin");
        assert_eq!(listed.anchors_team, descriptor.anchors_team());
        assert_eq!(listed.team_authority, descriptor.has_team_authority());
        assert_eq!(listed.can_spawn, descriptor.can_spawn());
    }
    let overlay = view
        .kinds
        .iter()
        .find(|kind| kind.kind_id == "rust-backend-dev")
        .expect("overlay listed");
    assert_eq!(overlay.origin, "overlay");
    assert!(!overlay.anchors_team && !overlay.team_authority && !overlay.can_spawn);

    assert_eq!(view.rejected.len(), 1);
    assert_eq!(view.rejected[0].reason, "capability_escalation");
    assert!(view.rejected[0].detail.contains("team_authority"));
}

#[test]
fn a_narrowed_view_keeps_only_the_requested_occasion() {
    let view = inspect::show(
        &OverlayCatalog::empty(),
        AgentRole::Worker,
        None,
        Some("You are Mira."),
        Some(Occasion::WorkerLaunchIdentity),
    )
    .expect("worker view");

    assert_eq!(view.occasions.len(), 1);
    assert_eq!(
        view.occasions[0].occasion,
        Occasion::WorkerLaunchIdentity.id()
    );
    assert_eq!(view.occasions[0].text, "You are Mira.");
    assert_eq!(
        view.guidance,
        format!("{}\n\nYou are Mira.", crate::prompts::worker_suffix()),
        "narrowing an occasion never narrows the resolved guidance"
    );
}

#[test]
fn a_roleless_pair_renders_no_view() {
    assert!(inspect::show(
        &OverlayCatalog::empty(),
        AgentRole::Manager,
        None,
        Some("persona"),
        None
    )
    .is_none());
}
