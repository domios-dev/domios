use super::*;

const OPAQUE_WORKSPACE: &str = "ws_target_7";
const OPAQUE_ORCHESTRATOR: &str = "provider-alpha";
const OPAQUE_PLANNER: &str = "provider-beta";

fn refusal(shape: CorrectiveShape) -> GeneralAgentTeamRefusal {
    GeneralAgentTeamRefusal::new(
        "tab_general",
        "general_agent",
        MembershipAttempt::Join,
        shape,
    )
}

#[test]
fn general_agent_team_guard_refusal_carries_the_constant_violation_id() {
    let refusal = refusal(CorrectiveShape::default());
    assert_eq!(
        refusal.violation_id,
        GENERAL_AGENT_TEAM_MEMBERSHIP_VIOLATION
    );
    assert_eq!(
        refusal.data()["violation_id"],
        GENERAL_AGENT_TEAM_MEMBERSHIP_VIOLATION
    );
}

#[test]
fn general_agent_team_guard_refusal_names_the_caller_and_its_kind() {
    let refusal = refusal(CorrectiveShape::default());
    assert!(refusal.reason().contains(&refusal.tab_id));
    assert!(refusal.reason().contains(&refusal.role_kind_id));
    assert_eq!(refusal.data()["tab_id"], refusal.tab_id);
    assert_eq!(refusal.data()["role_kind_id"], refusal.role_kind_id);
}

#[test]
fn general_agent_team_guard_corrective_command_substitutes_the_supplied_values() {
    let shape = CorrectiveShape::in_workspace(Some(OPAQUE_WORKSPACE))
        .with_agents(Some(OPAQUE_ORCHESTRATOR), Some(OPAQUE_PLANNER));
    let command = refusal(shape).corrective_command();
    for supplied in [OPAQUE_WORKSPACE, OPAQUE_ORCHESTRATOR, OPAQUE_PLANNER] {
        assert!(command.contains(supplied), "{command}");
    }
    assert!(!command.contains('<'), "no placeholder residue: {command}");
}

#[test]
fn general_agent_team_guard_unknown_values_stay_placeholders_not_empty_holes() {
    let refusal = refusal(CorrectiveShape::in_workspace(Some("   ")));
    let command = refusal.corrective_command();
    assert!(command.contains(WORKSPACE_PLACEHOLDER), "{command}");
    assert!(command.contains(PROVIDER_PLACEHOLDER), "{command}");
    assert!(
        !command.contains("  "),
        "no collapsed empty argument: {command}"
    );
}

#[test]
fn general_agent_team_guard_planner_falls_back_to_the_requested_orchestrator_provider() {
    let shape = CorrectiveShape::in_workspace(Some(OPAQUE_WORKSPACE))
        .with_agents(Some(OPAQUE_ORCHESTRATOR), None);
    let command = refusal(shape).corrective_command();
    assert_eq!(command.matches(OPAQUE_ORCHESTRATOR).count(), 2, "{command}");
}

#[test]
fn general_agent_team_guard_rendered_refusal_leads_with_the_id_and_ends_with_the_command() {
    let shape = CorrectiveShape::in_workspace(Some(OPAQUE_WORKSPACE))
        .with_agents(Some(OPAQUE_ORCHESTRATOR), Some(OPAQUE_PLANNER));
    let refusal = refusal(shape);
    let rendered = refusal.to_string();
    assert!(rendered.starts_with(GENERAL_AGENT_TEAM_MEMBERSHIP_VIOLATION));
    assert!(rendered.ends_with(&refusal.corrective_command()));
    assert_eq!(String::from(refusal.clone()), rendered);
}

#[test]
fn general_agent_team_guard_spawn_example_targets_the_requested_workspace() {
    let shape = CorrectiveShape::in_workspace(Some(OPAQUE_WORKSPACE))
        .with_agents(Some(OPAQUE_ORCHESTRATOR), None);
    let spawn = shape.spawn_orchestrator_command();
    assert!(spawn.contains(OPAQUE_WORKSPACE), "{spawn}");
    assert!(spawn.contains(OPAQUE_ORCHESTRATOR), "{spawn}");
}

#[test]
fn general_agent_team_guard_attempt_is_machine_readable_on_both_doors() {
    let join = GeneralAgentTeamRefusal::new(
        "tab_general",
        "general_agent",
        MembershipAttempt::Join,
        CorrectiveShape::default(),
    );
    let exercise = GeneralAgentTeamRefusal::new(
        "tab_general",
        "general_agent",
        MembershipAttempt::ExerciseMembership,
        CorrectiveShape::default(),
    );
    assert_eq!(join.violation_id, exercise.violation_id);
    assert_ne!(join.data()["attempt_id"], exercise.data()["attempt_id"]);
}
