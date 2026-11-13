use super::*;

#[test]
fn prompt_placeholders_are_fully_substituted() {
    let spawn_cmd = "fixture-spawn-command";
    let general_cli = "fixture-cli-reference";
    let mut composed: Vec<(&str, String)> = crate::role_preset::builtin_presets()
        .into_iter()
        .map(|preset| (preset.0, preset.1.guidance()))
        .collect();
    composed.extend([
        (
            "orchestrator_first_message",
            orchestrator_first_message("fixture-tab", "fixture-team", Some("fixture-plan")),
        ),
        (
            "orchestrator_first_message_no_plan",
            orchestrator_first_message("fixture-tab", "fixture-team", None),
        ),
        ("peer_pairing_mandate", peer_pairing_mandate_full(spawn_cmd)),
        (
            "general_agent",
            GENERAL_AGENT_TEMPLATE.replace("{codeterm_cli}", general_cli),
        ),
    ]);

    for (name, rendered) in composed {
        for slot in [
            "{planning_compaction_contract}",
            "{team_creation_contract}",
            "{task_sequencing_contract}",
            "{plan_task_flags}",
            "{test_author_guidance}",
            "{task_contract_line}",
            "{task_routing_line}",
            "{roleless_draft_rejection}",
            "{empty_role_guidance_rejection}",
            "{child_pty_recovery_trigger}",
            "{cartographer_contract_notice}",
            "{orchestrator_suffix}",
            "{planner_spawn_cmd}",
            "{spawn_cmd}",
            "{codeterm_cli}",
        ] {
            assert!(!rendered.contains(slot), "{name} retains {slot}");
        }
    }

    let first_message =
        orchestrator_first_message("fixture-tab", "fixture-team", Some("fixture-plan"));
    assert!(first_message.contains("fixture-tab"));
    assert!(first_message.contains("fixture-team"));
    assert!(first_message.contains("fixture-plan"));
    assert!(peer_pairing_mandate_full(spawn_cmd).contains(spawn_cmd));
    assert!(GENERAL_AGENT_TEMPLATE
        .replace("{codeterm_cli}", general_cli)
        .contains(general_cli));
}

/// A custom manager whose own prose is the longest default role's, composed in the default
/// Team, so the generic manager text and the Team listing are measured too.
fn composed_custom_manager() -> String {
    use codeterm_types::agent::AgentRole;
    let mut team = crate::builtin_roles::default_registrations("team_fixture");
    let longest = team
        .iter()
        .map(|registration| registration.instructions.clone())
        .max_by_key(String::len)
        .unwrap_or_default();
    let mut custom = team[0].clone();
    custom.name = "fixture_manager".into();
    custom.label = custom.name.clone();
    custom.instructions = longest;
    team.push(custom);
    crate::role_preset::resolve_in(&team, Some(AgentRole::Manager), Some("fixture_manager"), None)
        .expect("custom manager preset")
        .contract_guidance()
}

#[test]
fn composed_prompt_stays_within_size_bound() {
    let presets = crate::role_preset::builtin_presets()
        .into_iter()
        .map(|(_, preset)| preset.contract_guidance());
    for rendered in presets.chain([
        composed_custom_manager(),
        GENERAL_AGENT_TEMPLATE.replace("{codeterm_cli}", "fixture-cli-reference"),
    ]) {
        assert!(rendered.replace("\r\n", "\n").len() <= COMPOSED_ROLE_PROMPT_MAX_BYTES);
    }
}

#[test]
fn reflection_block_substitutes_the_supplied_values() {
    use codeterm_plan::service_plan::reflection::ReflectionWindow;

    let filled = reflection_block(
        "fixture-trigger-unique",
        &ReflectionWindow {
            tasks_closed: 317,
            bounces: 419,
            gate_leaks: 523,
        },
    );
    assert!(filled.contains("fixture-trigger-unique"));
    assert!(filled.contains("317"));
    assert!(filled.contains("419"));
    assert!(filled.contains("523"));
    assert!(!filled.contains("{trigger}") && !filled.contains("{closed}"));
}
