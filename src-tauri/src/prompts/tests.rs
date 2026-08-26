//! Prompt-fragment tests. Split out of `prompts.rs` to keep the source file
//! inside the repository size gate; the module contract is unchanged.

use super::*;

/// The committed clap manifest is the authority on which commands exist.
/// Generated artifact — read here, never written.
const CLI_MANIFEST: &str = include_str!("../../../docs/cli-manifest.json");

/// Every prompt fragment that teaches CLI grammar, with the file it is
/// loaded from so a failure names something a human can open.
fn cli_teaching_fragments() -> [(&'static str, &'static str); 8] {
    [
        ("general_agent.md", GENERAL_AGENT_TEMPLATE),
        ("codeterm_rule.md", CODETERM_RULE),
        ("codeterm_cli.md", CODETERM_CLI),
        ("planner_suffix.md", PLANNER_SUFFIX),
        ("orchestrator_suffix.md", ORCHESTRATOR_SUFFIX),
        ("worker_suffix.md", WORKER_SUFFIX),
        ("team_creation_contract.md", TEAM_CREATION_CONTRACT),
        ("agent_codeterm_primer.md", AGENT_CODETERM_PRIMER),
    ]
}

fn subcommand<'a>(node: &'a serde_json::Value, name: &str) -> Option<&'a serde_json::Value> {
    node["subcommands"].as_array()?.iter().find(|child| {
        child["name"] == *name
            || child["aliases"]
                .as_array()
                .is_some_and(|aliases| aliases.iter().any(|alias| alias == name))
    })
}

fn is_leaf(node: &serde_json::Value) -> bool {
    node["subcommands"]
        .as_array()
        .is_none_or(|children| children.is_empty())
}

/// A bare command word, or a `list|show` / `get/set` alternation of them.
fn command_words(token: &str) -> Option<Vec<&str>> {
    let words: Vec<&str> = token.split(['|', '/']).filter(|w| !w.is_empty()).collect();
    let bare = |w: &str| {
        w.starts_with(|c: char| c.is_ascii_lowercase())
            && w.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    };
    (!words.is_empty() && words.iter().copied().all(bare)).then_some(words)
}

/// Lines and backtick spans are where a fragment prescribes a runnable
/// command. Prose that merely says "codeterm" is not a reference.
fn command_spans(text: &str) -> Vec<&str> {
    let mut spans = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("codeterm ") {
            spans.push(trimmed);
        }
        for (index, part) in trimmed.split('`').enumerate() {
            if index % 2 == 1 {
                spans.push(part.trim());
            }
        }
    }
    spans
}

/// Resolve every command reference in one span, appending a human-readable
/// complaint for each reference the manifest cannot account for.
fn check_span(root: &serde_json::Value, span: &str, problems: &mut Vec<String>) {
    let tokens: Vec<&str> = span.split_whitespace().collect();
    for (index, token) in tokens.iter().enumerate() {
        if *token != "codeterm" || index + 1 >= tokens.len() {
            continue;
        }
        if command_words(tokens[index + 1]).is_none() {
            continue; // `codeterm <cmd> --help`, a bare binary mention, prose
        }
        let mut node = root;
        let mut path: Vec<&str> = Vec::new();
        let mut cursor = index + 1;
        let mut alternation = false;
        let mut unknown = false;
        while cursor < tokens.len() && !alternation && tokens[cursor] != "codeterm" {
            let Some(words) = command_words(tokens[cursor]) else {
                break;
            };
            let resolved: Vec<Option<&serde_json::Value>> =
                words.iter().map(|word| subcommand(node, word)).collect();
            if resolved.iter().any(Option::is_none) {
                if is_leaf(node) {
                    break; // a positional argument of a leaf command
                }
                path.push(tokens[cursor]);
                problems.push(format!("unknown command `codeterm {}`", path.join(" ")));
                unknown = true;
                break;
            }
            node = resolved[0].expect("resolved above");
            path.push(tokens[cursor]);
            cursor += 1;
            alternation = words.len() > 1;
        }
        if unknown {
            continue;
        }
        if !alternation && !is_leaf(node) {
            if let Some(next) = tokens.get(cursor) {
                if !next.starts_with('-')
                    && !next.starts_with('<')
                    && *next != "..."
                    && *next != "codeterm"
                {
                    problems.push(format!(
                        "`codeterm {}` is a command group but is used as if it took arguments",
                        path.join(" ")
                    ));
                }
            }
        }
    }
}

/// Every long flag the manifest accepts on one command node, aliases
/// included. Positionals are not flags and never satisfy a `--x` reference.
fn flag_names(node: &serde_json::Value) -> Vec<&str> {
    let Some(args) = node["args"].as_array() else {
        return Vec::new();
    };
    args.iter()
        .filter(|arg| arg["positional"].as_bool() != Some(true))
        .flat_map(|arg| {
            arg["name"].as_str().into_iter().chain(
                arg["aliases"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|alias| alias.as_str()),
            )
        })
        .collect()
}

/// A `--long-flag` reference, unwrapped from the optional-argument and
/// punctuation decoration prompts write around it (`[--model <model>]`).
fn long_flag(token: &str) -> Option<&str> {
    let flag = token
        .trim_matches(|c| matches!(c, '[' | ']' | '(' | ')' | ',' | '.' | '`' | '\'' | '"'))
        .strip_prefix("--")?
        .split('=')
        .next()?;
    (!flag.is_empty()
        && flag
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
    .then_some(flag)
}

/// Where one command reference stops and the next begins. Without these a
/// later command's flags get charged to the earlier command in the span.
fn ends_command(token: &str) -> bool {
    matches!(token, "codeterm" | "·" | "&&" | "||" | ";" | "|")
}

/// Prompts wrap commands in backticks and sentence punctuation. Strip both
/// so a decorated token still anchors and still resolves as a command word.
fn command_tokens(span: &str) -> Vec<&str> {
    span.split_whitespace()
        .map(|token| {
            token.trim_matches(|c| matches!(c, '`' | ';' | ',' | '.' | '(' | ')' | '"' | '\''))
        })
        .filter(|token| !token.is_empty())
        .collect()
}

/// Composed briefs write commands mid-sentence ("then run: codeterm …"),
/// which the line-start rule of [`command_spans`] misses. Anchor on the
/// first `codeterm` and stop at the closing backtick so trailing prose is
/// never mistaken for a positional argument.
fn brief_command_spans(text: &str) -> Vec<&str> {
    text.lines()
        .filter_map(|line| {
            let command = &line[line.find("codeterm ")?..];
            Some(command.find('`').map_or(command, |end| &command[..end]))
        })
        .collect()
}

/// Resolve each command reference in one span, then check every long flag
/// that follows it against that command node plus the global flags. A
/// prompt that teaches `plan create --team` fails here, not in production.
fn check_span_flags(root: &serde_json::Value, span: &str, problems: &mut Vec<String>) {
    let globals = flag_names(root);
    let tokens = command_tokens(span);
    let mut index = 0;
    while index < tokens.len() {
        if tokens[index] != "codeterm" {
            index += 1;
            continue;
        }
        let mut node = root;
        let mut path: Vec<&str> = Vec::new();
        let mut cursor = index + 1;
        while cursor < tokens.len() {
            let Some(words) = command_words(tokens[cursor]) else {
                break;
            };
            if words.len() > 1 {
                break;
            }
            let Some(child) = subcommand(node, words[0]) else {
                break;
            };
            node = child;
            path.push(tokens[cursor]);
            cursor += 1;
        }
        if path.is_empty() {
            index = cursor.max(index + 1);
            continue;
        }
        let allowed = flag_names(node);
        while cursor < tokens.len() && !ends_command(tokens[cursor]) {
            if let Some(flag) = long_flag(tokens[cursor]) {
                if flag != "help" && !allowed.contains(&flag) && !globals.contains(&flag) {
                    problems.push(format!(
                        "`codeterm {}` has no `--{flag}` flag",
                        path.join(" ")
                    ));
                }
            }
            cursor += 1;
        }
        index = cursor;
    }
}

/// A rename that strands a prompt fails here instead of teaching agents a
/// dead command. Resolution against the manifest, never prompt wording.
#[test]
fn prompt_cli_references_exist_in_manifest() {
    let root: serde_json::Value =
        serde_json::from_str(CLI_MANIFEST).expect("cli-manifest.json must parse");

    let mut problems: Vec<String> = Vec::new();
    for (name, fragment) in cli_teaching_fragments() {
        for span in command_spans(fragment) {
            let before = problems.len();
            check_span(&root, span, &mut problems);
            check_span_flags(&root, span, &mut problems);
            for problem in problems.iter_mut().skip(before) {
                *problem = format!("{name}: {problem}");
            }
        }
    }

    assert!(
        problems.is_empty(),
        "prompts teach commands the CLI manifest does not have. If the command \
         is new, docs/cli-manifest.json needs regeneration:\n{}",
        problems.join("\n")
    );
}

/// The onboarding briefs are composed at runtime, so they escape the
/// fragment sweep above. A brief that teaches a dead command or a flag the
/// verb does not take strands the very first move an Orchestrator makes.
#[test]
fn onboarding_briefs_teach_only_commands_the_manifest_has() {
    let root: serde_json::Value =
        serde_json::from_str(CLI_MANIFEST).expect("cli-manifest.json must parse");

    let mut problems: Vec<String> = Vec::new();
    for (name, rendered) in [
        (
            "orchestrator_first_message",
            orchestrator_first_message("tab-x", "team-y", Some("pl_z")),
        ),
        (
            "orchestrator_first_message_no_plan",
            orchestrator_first_message("tab-x", "team-y", None),
        ),
        (
            "planner_onboarding_task",
            planner_onboarding_task("Orion", "tab-x", "pl_z"),
        ),
        (
            "planner_onboarding_task_no_plan",
            planner_onboarding_task_no_plan("Orion", "tab-x"),
        ),
    ] {
        let spans = command_spans(&rendered)
            .into_iter()
            .chain(brief_command_spans(&rendered));
        for span in spans {
            let before = problems.len();
            check_span(&root, span, &mut problems);
            check_span_flags(&root, span, &mut problems);
            for problem in problems.iter_mut().skip(before) {
                *problem = format!("{name}: {problem}");
            }
        }
    }

    assert!(
        problems.is_empty(),
        "onboarding briefs teach commands the CLI manifest does not have:\n{}",
        problems.join("\n")
    );
}

/// `plan create` has no `--team`, so binding is its own verb. Pinned by
/// constant and by the ids this test supplies, never by prompt wording.
#[test]
fn no_plan_brief_creates_then_adopts_before_spawning_the_planner() {
    let brief = orchestrator_first_message("tab-x", "team-y", None);

    let create = brief
        .find(ORCH_ONBOARD_CREATE_STEP)
        .expect("brief must teach the create step");
    let adopt = brief
        .find(ORCH_ONBOARD_ADOPT_STEP)
        .expect("brief must teach the adopt step");
    let spawn = brief
        .find("codeterm agent spawn")
        .expect("brief must teach the planner spawn");
    assert!(create < adopt && adopt < spawn, "steps must stay ordered");

    let create_line = brief[create..]
        .lines()
        .next()
        .expect("the create step is one line");
    assert!(
        !create_line.contains("--team"),
        "`plan create` takes no --team: {create_line}"
    );
    assert!(brief.contains(&format!("{ORCH_ONBOARD_ADOPT_STEP} team-y")));
    assert!(brief.contains("--parent-tab tab-x"));
}

#[test]
fn planner_suffix_substitutes_the_test_author_guidance_constant() {
    let suffix = planner_suffix();

    assert!(suffix.contains(TEST_AUTHOR_GUIDANCE));
    assert!(!suffix.contains("{test_author_guidance}"));
}

/// Worker briefs compose three fragments. Their wording belongs to the .md
/// files; only the composition is a mechanism worth asserting here.
#[test]
fn worker_suffix_composes_its_discipline_fragments() {
    let rendered = worker_suffix();
    assert!(!TESTING_DISCIPLINE.trim().is_empty());
    assert!(!QA_PIPELINE_DISCIPLINE.trim().is_empty());
    assert!(rendered.contains(TESTING_DISCIPLINE.trim()));
    assert!(rendered.contains(QA_PIPELINE_DISCIPLINE.trim()));
    assert!(!rendered.contains('{'), "unsubstituted placeholder remains");
}

#[test]
fn cron_skill_prompts_keep_their_substitution_slots() {
    assert!(CRON_SKILL_HINT.contains("{skill_path}"));
    assert!(CRON_SKILL_HINT.contains("{skill_slug}"));
    assert!(CRON_SKILL_MISSING.contains("{skill_slug}"));
}

#[test]
fn orchestration_guidance_uses_only_complete_document_reads() {
    const ORCHESTRATION_DOC: &str = include_str!("../../../docs/ORCHESTRATION.md");
    const PLANNER_DOC: &str = include_str!("../../../docs/PLANNER.md");
    const PLAN_EDITING_DOC: &str = include_str!("../../../docs/PLAN-EDITING.md");
    let surfaces = [
        ORCHESTRATOR_ONBOARDING,
        PLANNER_SUFFIX,
        CODETERM_CLI,
        ORCHESTRATION_DOC,
        PLANNER_DOC,
        PLAN_EDITING_DOC,
    ];

    for surface in surfaces {
        for fragment in surface.split('`') {
            let command = fragment.trim();
            if command.starts_with("codeterm docs") {
                for removed in ["--head", "--section", "--full"] {
                    assert!(
                        !command.contains(removed),
                        "removed docs mode remains: {command}"
                    );
                }
            }
        }
    }
}

/// The docs-discovery rule has exactly one home, asserted by command form
/// rather than by the sentence that introduces it.
#[test]
fn codeterm_docs_rule_has_exactly_one_home() {
    const DISCOVERY_FORM: &str = "codeterm docs <NAME>";

    assert_eq!(CODETERM_CLI.matches(DISCOVERY_FORM).count(), 1);
    let general = GENERAL_AGENT_TEMPLATE.replace("{codeterm_cli}", CODETERM_CLI);
    assert_eq!(general.matches(DISCOVERY_FORM).count(), 1);
    for role_surface in [
        ORCHESTRATOR_ONBOARDING,
        ORCHESTRATOR_SUFFIX,
        PLANNER_SUFFIX,
        WORKER_SUFFIX,
        AGENT_CODETERM_PRIMER,
        WORKER_CODETERM_CONTEXT,
    ] {
        assert_eq!(
            role_surface.matches(DISCOVERY_FORM).count(),
            0,
            "role instruction restates the docs rule instead of pointing at its one home"
        );
    }
}

#[test]
fn agent_pad_rule_has_exactly_one_source() {
    // The fragment is an opaque token here. Asserting its wording would make
    // rewriting the rule fail a test that has nothing to say about wording.
    let rule = AGENT_PAD_RULE.trim();
    assert!(!rule.is_empty());

    for surface in [
        CODETERM_CLI,
        ORCHESTRATOR_ONBOARDING,
        ORCHESTRATOR_SUFFIX,
        PLANNER_SUFFIX,
        WORKER_SUFFIX,
        AGENT_CODETERM_PRIMER,
        WORKER_CODETERM_CONTEXT,
    ] {
        assert!(
            !surface.contains(rule),
            "the AgentPad rule must live only in AGENT_PAD_RULE"
        );
    }
}

#[test]
fn selected_instruction_references_name_whole_documents() {
    const ORCHESTRATION_DOC: &str = include_str!("../../../docs/ORCHESTRATION.md");
    const PLANNER_DOC: &str = include_str!("../../../docs/PLANNER.md");
    const PLAN_EDITING_DOC: &str = include_str!("../../../docs/PLAN-EDITING.md");
    let surfaces = [
        ORCHESTRATOR_ONBOARDING,
        PLANNER_SUFFIX,
        CODETERM_CLI,
        ORCHESTRATION_DOC,
        PLANNER_DOC,
        PLAN_EDITING_DOC,
    ];
    for surface in surfaces {
        for fragment in surface.split('`') {
            let command = fragment.trim();
            if !command.starts_with("codeterm docs ")
                || command.contains("<TOPIC>")
                || command.contains("<NAME>")
                || command == "codeterm docs --help"
            {
                continue;
            }
            let document = command
                .strip_prefix("codeterm docs ")
                .and_then(|rest| rest.split_whitespace().next())
                .unwrap_or_default();
            assert!(
                [
                    "ORCHESTRATION",
                    "ORCHESTRATION-OPERATIONS",
                    "PLANNER",
                    "PLAN-EDITING",
                    "LANES",
                    "BOUNCE",
                    "TEST-AUTHORING",
                    "ROUTING",
                    "MESH",
                    "MEMORY",
                    "SECRETS",
                    "API-CLI",
                ]
                .contains(&document),
                "instruction reference names an unknown document: {command}"
            );
            assert_eq!(command, format!("codeterm docs {document}"));
        }
    }
}

#[test]
fn peer_pairing_mandate_block_fills_spawn_cmd() {
    let filled = peer_pairing_mandate_block("codeterm agent spawn claude --role Planner");
    assert!(filled.contains("codeterm agent spawn claude --role Planner"));
    assert!(!filled.contains("{spawn_cmd}"));
    assert!(!PEER_WAIVER_HINT.trim().is_empty());
}

#[test]
fn peer_pairing_mandate_full_fills_spawn_cmd_and_carries_charter() {
    let full = peer_pairing_mandate_full("codeterm agent spawn claude --role Planner");
    assert!(full.contains("codeterm agent spawn claude --role Planner"));
    assert!(!full.contains("{spawn_cmd}"));
    assert!(full.contains(PEER_PLANNER_CHARTER_LINE));
    assert!(full.contains(PEER_ORCH_DEFERENCE_LINE));
}

#[test]
fn planner_onboarding_task_carries_role_and_toolset_from_codeterm() {
    let t = planner_onboarding_task("Opus Orchestrator", "tab_orch_9", "pl_abc");
    assert!(t.contains("pl_abc"), "names the plan");
    assert!(t.contains("Opus Orchestrator"), "names the orchestrator");
    assert!(t.contains(PEER_PLANNER_CHARTER_LINE), "reuses the charter");
    assert!(t.contains(PEER_ORCH_DEFERENCE_LINE), "reuses the deference");
}

#[test]
fn planner_onboarding_task_no_plan_carries_pair_hold_state_without_plan_id() {
    let t = planner_onboarding_task_no_plan("Opus Orchestrator", "tab_orch_9");
    assert!(t.contains("Opus Orchestrator"));
    // The real invariant: the no-plan variant must never leak a plan id.
    assert!(!t.contains("pl_"));
}

#[test]
fn orchestrator_suffix_embeds_the_canonical_manual() {
    let rendered = orchestrator_suffix();
    assert!(rendered.ends_with(ORCHESTRATION_DOC.trim()));
    assert!(!rendered.contains("{planning_compaction_contract}"));
}

// Rich-rendering delivery is a composition fact, so it is asserted where the
// composition lives — service::orchestrator::launch::tests. The two tests that
// used to sit here were named after the primer and the worker context but only
// asserted phrases inside RICH_RENDERING_GUIDANCE, so they passed regardless of
// whether either recipient received it.

/// One source, every surface that cuts or judges a task graph. The fragment
/// is an opaque token here; its wording belongs to the .md file.
#[test]
fn planning_compaction_contract_reaches_both_planning_surfaces() {
    let contract = PLANNING_COMPACTION_CONTRACT.trim();
    assert!(!contract.is_empty());

    // Both planning surfaces are system prompts; the onboarding first-message
    // no longer carries methodology, so it is intentionally absent here.
    for rendered in [orchestrator_suffix(), planner_suffix()] {
        assert!(
            rendered.contains(contract),
            "planning contract not delivered"
        );
        assert!(!rendered.contains("{planning_compaction_contract}"));
    }
}

/// Round-trips values this test supplies, so it proves substitution rather
/// than a phrase the prompt happens to contain.
#[test]
fn prompt_placeholders_are_fully_substituted() {
    let spawn_cmd = "codeterm agent spawn claude --role Planner";
    let composed = [
        ("orchestrator_suffix", orchestrator_suffix()),
        ("planner_suffix", planner_suffix()),
        (
            "orchestrator_first_message",
            orchestrator_first_message("tab-x", "team-y", Some("pl_z")),
        ),
        (
            "orchestrator_first_message_no_plan",
            orchestrator_first_message("tab-x", "team-y", None),
        ),
        ("peer_pairing_mandate", peer_pairing_mandate_full(spawn_cmd)),
        (
            "general_agent",
            GENERAL_AGENT_TEMPLATE.replace("{codeterm_cli}", CODETERM_CLI),
        ),
    ];

    for (name, rendered) in composed {
        for slot in [
            "{planning_compaction_contract}",
            "{team_creation_contract}",
            "{test_author_guidance}",
            "{task_contract_line}",
            "{task_routing_line}",
            "{roleless_draft_rejection}",
            "{orchestrator_suffix}",
            "{planner_spawn_cmd}",
            "{spawn_cmd}",
            "{codeterm_cli}",
        ] {
            assert!(
                !rendered.contains(slot),
                "{name} still carries the unsubstituted slot {slot}"
            );
        }
    }

    // Round-trip: the ids this test supplied must survive substitution.
    let first_message = orchestrator_first_message("tab-x", "team-y", Some("pl_z"));
    assert!(first_message.contains("tab-x"));
    assert!(first_message.contains("team-y"));
    assert!(first_message.contains("pl_z"));
    assert!(peer_pairing_mandate_full(spawn_cmd).contains(spawn_cmd));
}

/// Delivery, asserted by constant identity. Fragments are opaque tokens
/// here: which recipient receives which, never their wording.
#[test]
fn role_fragments_reach_their_recipients() {
    let orchestrator = orchestrator_suffix();
    let planner = planner_suffix();
    let worker = worker_suffix();

    for (recipient, rendered, fragments) in [
        (
            "orchestrator",
            &orchestrator,
            vec![PLANNING_COMPACTION_CONTRACT, TEAM_CREATION_CONTRACT],
        ),
        (
            "planner",
            &planner,
            vec![PLANNING_COMPACTION_CONTRACT, TEAM_CREATION_CONTRACT],
        ),
        (
            "worker",
            &worker,
            vec![TESTING_DISCIPLINE, QA_PIPELINE_DISCIPLINE, WORKER_SUFFIX],
        ),
    ] {
        for fragment in fragments {
            let token = fragment.trim();
            assert!(!token.is_empty(), "an empty fragment proves nothing");
            assert_eq!(
                rendered.matches(token).count(),
                1,
                "{recipient} must receive each of its fragments exactly once"
            );
        }
    }

    // The Team-bound creation contract has one source, not a copy per role.
    let contract = TEAM_CREATION_CONTRACT.trim();
    for surface in [
        WORKER_SUFFIX,
        AGENT_CODETERM_PRIMER,
        WORKER_CODETERM_CONTEXT,
    ] {
        assert!(!surface.contains(contract));
    }
}

#[test]
fn composed_prompt_stays_within_size_bound() {
    for (name, rendered) in [
        ("orchestrator_suffix", orchestrator_suffix()),
        ("planner_suffix", planner_suffix()),
        ("worker_suffix", worker_suffix()),
        (
            "general_agent",
            GENERAL_AGENT_TEMPLATE.replace("{codeterm_cli}", CODETERM_CLI),
        ),
    ] {
        assert!(
            rendered.len() <= COMPOSED_ROLE_PROMPT_MAX_BYTES,
            "{name} is {} bytes, over COMPOSED_ROLE_PROMPT_MAX_BYTES ({})",
            rendered.len(),
            COMPOSED_ROLE_PROMPT_MAX_BYTES
        );
    }
}

#[test]
fn planner_suffix_embeds_the_canonical_manual() {
    let rendered = planner_suffix();
    assert!(rendered.ends_with(PLANNER_DOC.trim()));
    assert!(!rendered.contains("{planning_compaction_contract}"));
    assert!(!rendered.contains("{test_author_guidance}"));
}

#[test]
fn milestone_gate_constants_wire_into_orchestration_ladder() {
    const ORCHESTRATION_DOC: &str = include_str!("../../../docs/ORCHESTRATION-OPERATIONS.md");

    // Suggestion fills both placeholders and names the record command.
    let msg = milestone_gate_suggestion("phase-1", "Phase 1");
    assert!(msg.contains("group Phase 1 complete"));
    assert!(msg.contains("codeterm plan qa next --group phase-1"));
    assert!(!msg.contains("{label}") && !msg.contains("{key}"));

    // The doc must quote the shared constant verbatim — asserted by constant,
    // not by quoted prose, so rewording the ladder updates one place.
    assert!(ORCHESTRATION_DOC.contains(MILESTONE_GATE_LADDER));
    assert_eq!(REGRESSION_MILESTONE_KIND, "regression");
    assert!(!MILESTONE_GATE_HEARTBEAT_TITLE.trim().is_empty());
}

#[test]
fn reflection_block_fills_counts_and_routes_to_existing_primitives() {
    use crate::service::plan::reflection::ReflectionWindow;
    let filled = reflection_block(
        "group Wave 1 closed",
        &ReflectionWindow {
            tasks_closed: 3,
            bounces: 1,
            gate_leaks: 2,
        },
    );
    assert!(filled.contains("group Wave 1 closed"));
    assert!(filled.contains("tasks closed: 3"));
    assert!(filled.contains("bounces: 1"));
    assert!(filled.contains("gate leaks: 2"));
    assert!(!filled.contains("{trigger}") && !filled.contains("{closed}"));
    assert!(!REFLECTION_HEARTBEAT_TITLE.trim().is_empty());
}

/// Was named for GA routing while asserting template phrases; the mechanism
/// it can actually own is the CLI-reference injection seam.
#[test]
fn general_agent_template_receives_the_cli_reference() {
    assert!(GENERAL_AGENT_TEMPLATE.contains("{codeterm_cli}"));

    let general = GENERAL_AGENT_TEMPLATE.replace("{codeterm_cli}", CODETERM_CLI);
    assert!(general.contains(CODETERM_CLI));
    assert!(!general.contains("{codeterm_cli}"));
}

#[test]
fn reflection_block_runs_velocity_and_cost_sweep() {
    assert!(REFLECTION_BLOCK.contains("VELOCITY AND COST sweep"));
    assert!(REFLECTION_BLOCK.contains("no longer earn their priority"));
    assert!(REFLECTION_BLOCK.contains("cheaper model"));
}

#[test]
fn retrospective_nudge_graduates_skills_and_scripts_via_reviewed_lane() {
    assert!(PLAN_RETROSPECTIVE_NUDGE.contains("codeterm mem skill"));
    assert!(PLAN_RETROSPECTIVE_NUDGE.contains(".agents/skills/"));
    assert!(PLAN_RETROSPECTIVE_NUDGE.contains("scripts/"));
    assert!(PLAN_RETROSPECTIVE_NUDGE.contains("reviewed lane"));
}

#[test]
fn plan_skills_worker_and_registry_lines_are_pointers() {
    assert!(PLAN_SKILLS_WORKER_LINE.contains("codeterm plan skill list"));
    assert!(PLAN_SKILLS_WORKER_LINE.contains("codeterm plan skill new"));
    assert!(PLAN_SKILLS_REGISTRY_LINE.contains("{n}"));
    assert!(PLAN_SKILLS_REGISTRY_LINE.contains("codeterm plan skill show <slug> | new | edit."));
}

/// The seeded `codeterm` skill body is compiled into codeterm-mem, which
/// `test-one app` cannot reach. Read the one file both crates load, so this
/// stays a single-crate gate on the same bytes mem seeds from.
const SEED_SKILL_CODETERM: &str = include_str!("../../prompts/seed/skill_codeterm.md");

/// The long flag the ask clause teaches, named once so the clause and the
/// manifest are compared against the same token rather than two spellings.
const ASK_FLAG: &str = "--choice";

/// Every surface that teaches an agent when to escalate to the user. The
/// primer reaches composed launch prompts through the seam already asserted
/// in service::orchestrator::prompt_compose::tests, so a clause present in
/// the fragment is a clause present in what an agent receives.
fn notify_escalation_surfaces() -> [(&'static str, &'static str); 3] {
    [
        ("codeterm_rule.md", CODETERM_RULE),
        ("agent_codeterm_primer.md", AGENT_CODETERM_PRIMER),
        ("seed/skill_codeterm.md", SEED_SKILL_CODETERM),
    ]
}

/// Delivery, by constant identity. The clause is an opaque token here: what
/// it says belongs to the .md files, and that each surface receives it once
/// is the only mechanism this test owns.
#[test]
fn notify_ask_guidance_reaches_every_escalation_surface() {
    let clause = NOTIFY_ASK_CLAUSE;
    assert!(
        !clause.trim().is_empty(),
        "an empty fragment proves nothing"
    );

    for (name, surface) in notify_escalation_surfaces() {
        assert_eq!(
            surface.matches(clause).count(),
            1,
            "{name} must carry the ask clause exactly once"
        );
    }
}

/// The clause names a flag; the committed manifest is the authority on
/// whether that flag exists. Guidance cannot document a flag the CLI lacks.
#[test]
fn notify_ask_guidance_names_a_flag_the_manifest_has() {
    let root: serde_json::Value =
        serde_json::from_str(CLI_MANIFEST).expect("cli-manifest.json must parse");
    let send = subcommand(&root, "notify")
        .and_then(|notify| subcommand(notify, "send"))
        .expect("the manifest must carry notify send");

    assert!(NOTIFY_ASK_CLAUSE.contains(ASK_FLAG));
    assert!(
        flag_names(send).contains(&ASK_FLAG.trim_start_matches('-')),
        "notify send must accept the flag the ask clause teaches"
    );
}

/// The addition is a clause on a restriction, never a replacement for it:
/// the surface that carries the clause must still state the rule that keeps
/// the escalation rare. Both are named by constant, not by their sentences.
#[test]
fn notify_ask_guidance_leaves_the_rare_escalation_rule_standing() {
    assert!(!NOTIFY_RARE_ESCALATION_RULE.trim().is_empty());
    assert_eq!(
        AGENT_CODETERM_PRIMER
            .matches(NOTIFY_RARE_ESCALATION_RULE)
            .count(),
        1
    );
    assert!(AGENT_CODETERM_PRIMER.contains(NOTIFY_ASK_CLAUSE));
}

/// Worker and Planner tabs never notify the user directly, so their role
/// surfaces must not gain the clause. A copy there would read as an
/// invitation to the one role the restriction excludes.
#[test]
fn notify_ask_guidance_stays_out_of_surfaces_that_never_notify() {
    for (name, surface) in surfaces_that_never_notify() {
        assert!(
            !surface.contains(NOTIFY_ASK_CLAUSE),
            "{name} must not restate the ask clause"
        );
    }
}

/// The roles the notify restriction excludes. Named once so the negative
/// control and the ask-clause control cannot drift apart.
fn surfaces_that_never_notify() -> [(&'static str, &'static str); 3] {
    [
        ("worker_suffix.md", WORKER_SUFFIX),
        ("worker_codeterm_context.md", WORKER_CODETERM_CONTEXT),
        ("planner_suffix.md", PLANNER_SUFFIX),
    ]
}

/// Delivery, by constant identity. What the relaxation says belongs to the .md
/// files; that every surface teaching notify receives it once is the mechanism
/// this test owns.
#[test]
fn verbose_completion_relaxation_reaches_every_notify_surface() {
    let clause = NOTIFY_VERBOSE_COMPLETION_CLAUSE;
    assert!(!clause.trim().is_empty(), "an empty fragment proves nothing");

    for (name, surface) in notify_escalation_surfaces() {
        assert_eq!(
            surface.matches(clause).count(),
            1,
            "{name} must carry the verbose completion clause exactly once"
        );
    }
}

/// The same surfaces must explain the `unseen` report, so an agent meeting it
/// under the id reads information rather than an error it should retry.
#[test]
fn verbose_completion_unseen_clause_reaches_every_notify_surface() {
    let clause = NOTIFY_UNSEEN_CLAUSE;
    assert!(!clause.trim().is_empty(), "an empty fragment proves nothing");

    for (name, surface) in notify_escalation_surfaces() {
        assert_eq!(
            surface.matches(clause).count(),
            1,
            "{name} must carry the unseen clause exactly once"
        );
    }
}

/// A relaxation, not an invitation. The permission cannot be delivered without
/// the restriction it carries, and the two rules that did not relax must still
/// stand where they stood. All three are named by constant, never by sentence.
#[test]
fn verbose_completion_leaves_the_surviving_restrictions_standing() {
    assert!(!NOTIFY_NO_RUNNING_COMMENTARY_RULE.trim().is_empty());
    assert!(
        NOTIFY_VERBOSE_COMPLETION_CLAUSE.contains(NOTIFY_NO_RUNNING_COMMENTARY_RULE),
        "the permission must carry its own restriction"
    );

    for (name, surface) in notify_escalation_surfaces() {
        assert_eq!(
            surface.matches(NOTIFY_NO_RUNNING_COMMENTARY_RULE).count(),
            1,
            "{name} must still forbid a running commentary"
        );
    }

    assert!(!NOTIFY_WORKER_PLANNER_SILENCE_RULE.trim().is_empty());
    assert_eq!(
        AGENT_CODETERM_PRIMER
            .matches(NOTIFY_WORKER_PLANNER_SILENCE_RULE)
            .count(),
        1,
        "the primer must still send Workers and Planners through their channel"
    );
    assert_eq!(
        AGENT_CODETERM_PRIMER
            .matches(NOTIFY_RARE_ESCALATION_RULE)
            .count(),
        1,
        "the two-case rule for the escalating levels must still stand"
    );
}

/// The relaxation is scoped to the surfaces that teach notify. A copy on a
/// Worker or Planner surface would read as permission to the roles excluded.
#[test]
fn verbose_completion_stays_out_of_surfaces_that_never_notify() {
    for (name, surface) in surfaces_that_never_notify() {
        assert!(
            !surface.contains(NOTIFY_VERBOSE_COMPLETION_CLAUSE),
            "{name} must not restate the verbose completion clause"
        );
        assert!(
            !surface.contains(NOTIFY_UNSEEN_CLAUSE),
            "{name} must not restate the unseen clause"
        );
    }
}

#[test]
fn every_prompt_md_has_a_loader() {
    // All top-level .md files in prompts/ that have an include_str!() loader.
    // Any file in the directory that is NOT in this set is an orphan — it is
    // compiled into the binary by nobody and misleads future editors.
    let loaded: std::collections::HashSet<&str> = [
        "worker_suffix.md",
        "testing_discipline.md",
        "qa_pipeline_discipline.md",
        "planner_suffix.md",
        "planning_compaction.md",
        "team_creation_contract.md",
        "bounce_unwinding_contract.md",
        "orchestrator_suffix.md",
        "orchestrator_onboarding.md",
        "manager_bootstrap.md",
        "team_bootstrap_starter.md",
        "cron_report_hint.md",
        "cron_complete_suffix.md",
        "cron_skill_hint.md",
        "cron_skill_missing.md",
        "codeterm_rule.md",
        "bridge_mem.md",
        "general_agent.md",
        "agent_codeterm_primer.md",
        "agent_pad_rule.md",
        "worker_codeterm_context.md",
        "rich_rendering_guidance.md",
        "codeterm_cli.md",
        "bootstrap_instructions.md",
        "worker_task_preamble.md",
        "plugin_agent_task_preamble.md",
        "mem_extraction.md",
        "mem_rerank.md",
        "dreaming_rem.md",
    ]
    .into_iter()
    .collect();

    let prompts_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("prompts");
    let orphans: Vec<String> = std::fs::read_dir(&prompts_dir)
        .expect("prompts/ must be readable")
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path().is_file() && e.path().extension().and_then(|x| x.to_str()) == Some("md")
        })
        .filter(|e| !loaded.contains(e.file_name().to_str().unwrap_or("")))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();

    assert!(
        orphans.is_empty(),
        "orphaned prompt files with no include_str!() loader: {:?}",
        orphans
    );
}

/// The seven fields the mandatory list held before the executor role joined it.
/// Named here so "nothing was lost in the move" is checkable against a baseline
/// rather than against the list under test.
const MANDATORY_FIELDS_BEFORE_THE_MOVE: [&str; 7] = [
    "a bounded body",
    "section links",
    "acceptance criteria",
    "a correctness vector",
    "an executable test plan",
    "an ownership boundary",
    "report expectations",
];

/// The defect was placement, not absence: the field sat in the routing sentence
/// beside three genuinely optional flags and inherited their optionality.
#[test]
fn executor_role_prompt_field_is_a_member_of_the_mandatory_list() {
    assert!(
        PLANNER_TASK_MANDATORY_FIELDS.contains(&PLANNER_TASK_EXECUTOR_ROLE_FIELD),
        "the executor role must be obliged by membership in the mandatory list"
    );
    assert!(
        !PLANNER_TASK_ROUTING_FLAGS.contains(&PLANNER_TASK_EXECUTOR_ROLE_FIELD),
        "a member of the optional routing set inherits its optionality"
    );

    let contract = planner_task_contract_line();
    assert!(contract.contains(PLANNER_TASK_EXECUTOR_ROLE_FIELD));

    let rendered = planner_suffix();
    assert_eq!(
        rendered.matches(contract.as_str()).count(),
        1,
        "the Planner surface must carry the mandatory-field fragment exactly once"
    );
    assert!(!rendered.contains("{task_contract_line}"));
}

/// A move, not a rewrite. Both lists must still hold every member they held.
#[test]
fn executor_role_prompt_move_drops_nothing_from_either_list() {
    for field in MANDATORY_FIELDS_BEFORE_THE_MOVE {
        assert!(
            PLANNER_TASK_MANDATORY_FIELDS.contains(&field),
            "the move dropped the mandatory field {field}"
        );
    }
    assert_eq!(
        PLANNER_TASK_MANDATORY_FIELDS.len(),
        MANDATORY_FIELDS_BEFORE_THE_MOVE.len() + 1,
        "the list gained the executor role and nothing else"
    );

    // Ruled out of the MANDATORY scope, not out of the product.
    for flag in ["`--model`", "`--personality`", "`--role-guidance`"] {
        assert!(
            PLANNER_TASK_ROUTING_FLAGS.contains(&flag),
            "the routing set lost {flag}"
        );
    }
    assert_eq!(PLANNER_TASK_ROUTING_FLAGS.len(), 3);

    let rendered = planner_suffix();
    for member in PLANNER_TASK_MANDATORY_FIELDS
        .iter()
        .chain(PLANNER_TASK_ROUTING_FLAGS)
    {
        assert!(
            rendered.contains(member),
            "the Planner surface no longer delivers {member}"
        );
    }
    assert!(!rendered.contains("{task_routing_line}"));
}

/// Delivery by constant identity: the duty is stated inside the instruction
/// where the Orchestrator already judges drafts, never as a separate rule.
#[test]
fn executor_role_prompt_rejection_duty_reaches_the_orchestrator_surface() {
    assert!(
        !ORCH_ROLELESS_DRAFT_REJECTION.trim().is_empty(),
        "an empty fragment proves nothing"
    );

    let rendered = orchestrator_suffix();
    assert_eq!(
        rendered.matches(ORCH_ROLELESS_DRAFT_REJECTION).count(),
        1,
        "the Orchestrator surface must carry the rejection duty exactly once"
    );
    assert!(!rendered.contains("{roleless_draft_rejection}"));
}

/// A rule about authoring tasks has no business in the prompt of an agent that
/// executes them. The Watcher is not a fourth surface to keep clean:
/// `builtin_guidance` hands it the Planner's, asserted here so the negative
/// control cannot silently start covering a surface that does not exist.
#[test]
fn executor_role_prompt_stays_off_the_surfaces_that_only_execute() {
    use codeterm_types::agent::role_kind::builtins;

    for (name, surface) in [
        ("worker_suffix", worker_suffix()),
        ("worker_codeterm_context", WORKER_CODETERM_CONTEXT.to_string()),
        ("worker_task_preamble", WORKER_TASK_PREAMBLE.to_string()),
        ("agent_codeterm_primer", AGENT_CODETERM_PRIMER.to_string()),
    ] {
        assert!(
            !surface.contains(PLANNER_TASK_EXECUTOR_ROLE_FIELD),
            "{name} must not carry the task-authoring field"
        );
        assert!(
            !surface.contains(ORCH_ROLELESS_DRAFT_REJECTION),
            "{name} must not carry the draft-rejection duty"
        );
    }

    let worker = crate::service::role_preset::builtin_guidance(builtins::WORKER)
        .expect("the Worker kind has bundled guidance");
    assert!(!worker.contains(PLANNER_TASK_EXECUTOR_ROLE_FIELD));
    assert!(!worker.contains(ORCH_ROLELESS_DRAFT_REJECTION));

    assert_eq!(
        crate::service::role_preset::builtin_guidance(builtins::WATCHER),
        crate::service::role_preset::builtin_guidance(builtins::PLANNER),
        "the Watcher has no surface of its own; it is handed the Planner's"
    );
}
