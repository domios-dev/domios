use super::*;

#[test]
fn prompt_placeholders_are_fully_substituted() {
    let spawn_cmd = "fixture-spawn-command";
    let general_cli = "fixture-cli-reference";
    let composed = [
        ("orchestrator_suffix", orchestrator_suffix()),
        ("planner_suffix", planner_suffix()),
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
    ];

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
    assert!(
        GENERAL_AGENT_TEMPLATE
            .replace("{codeterm_cli}", general_cli)
            .contains(general_cli)
    );
}

#[test]
fn composed_prompt_stays_within_size_bound() {
    for rendered in [
        orchestrator_suffix(),
        planner_suffix(),
        worker_suffix(),
        GENERAL_AGENT_TEMPLATE.replace("{codeterm_cli}", "fixture-cli-reference"),
    ] {
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
