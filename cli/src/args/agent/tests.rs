#[test]
fn agent_fork_and_model_force_parse() {
    let fork = Cli::try_parse_from(["codeterm", "agent", "fork", "--tab", "fixture"]).unwrap();
    assert!(
        matches!(fork.command,crate::Commands::Agent{action:AgentAction::Fork{tab:Some(tab)}} if tab=="fixture")
    );
    let switch = Cli::try_parse_from([
        "codeterm", "agent", "model", "--tab", "fixture", "opus", "--force",
    ])
    .unwrap();
    assert!(matches!(
        switch.command,
        crate::Commands::Agent {
            action: AgentAction::Model { force: true, .. }
        }
    ));
}

#[test]
fn agent_model_parses_speed_only_and_catalogue_listing() {
    let speed = Cli::try_parse_from([
        "codeterm", "agent", "model", "--tab", "tab-a", "--speed", "default",
    ])
    .unwrap();
    assert!(
        matches!(speed.command, crate::Commands::Agent { action: AgentAction::Model { name: None, speed: Some(value), .. } } if value == "default")
    );
    let list = Cli::try_parse_from([
        "codeterm",
        "agent",
        "model",
        "--tab",
        "tab-a",
        "--list-speeds",
    ])
    .unwrap();
    assert!(matches!(
        list.command,
        crate::Commands::Agent {
            action: AgentAction::Model {
                list_speeds: true,
                ..
            }
        }
    ));
    assert!(Cli::try_parse_from(["codeterm", "agent", "model", "--list-speeds", "opus"]).is_err());
}

#[test]
fn agent_model_parses_reasoning_effort_and_runtime_mode_without_model() {
    let parsed = Cli::try_parse_from([
        "codeterm",
        "agent",
        "model",
        "--tab",
        "tab-a",
        "--reasoning-effort",
        "high",
        "--runtime-mode",
        "auto",
    ])
    .unwrap();
    assert!(matches!(
        parsed.command,
        crate::Commands::Agent {
            action: AgentAction::Model {
                name: None,
                reasoning_effort: Some(effort),
                runtime_mode: Some(mode),
                ..
            }
        } if effort == "high" && mode == "auto"
    ));
    assert!(Cli::try_parse_from([
        "codeterm",
        "agent",
        "model",
        "--list-speeds",
        "--runtime-mode",
        "auto"
    ])
    .is_err());
}

#[test]
fn agent_turns_parses_bounded_window_and_rejects_invalid_limits() {
    let cli = Cli::try_parse_from([
        "codeterm", "agent", "turns", "--tab", "worker", "--last", "2", "--after", "101",
    ])
    .unwrap();
    assert!(matches!(
        cli.command,
        crate::Commands::Agent {
            action: AgentAction::Turns {
                last: 2,
                after: 101,
                ..
            }
        }
    ));
    assert!(
        Cli::try_parse_from(["codeterm", "agent", "turns", "--tab", "worker", "--last", "0"])
            .is_err()
    );
    assert!(
        Cli::try_parse_from(["codeterm", "agent", "turns", "--tab", "worker", "--last", "51"])
            .is_err()
    );
}

use super::*;
use crate::Cli;
use clap::{CommandFactory, Parser};

fn parse_report(args: &[&str]) -> Result<AgentAction, clap::Error> {
    let mut full = vec!["codeterm", "agent", "report"];
    full.extend_from_slice(args);
    let cli = Cli::try_parse_from(full)?;
    match cli.command {
        crate::Commands::Agent { action } => Ok(action),
        _ => panic!("expected agent subcommand"),
    }
}

fn parse_spawn(args: &[&str]) -> Result<AgentAction, clap::Error> {
    let mut full = vec!["codeterm", "agent", "spawn"];
    full.extend_from_slice(args);
    let cli = Cli::try_parse_from(full)?;
    match cli.command {
        crate::Commands::Agent { action } => Ok(action),
        _ => panic!("expected agent subcommand"),
    }
}

#[test]
fn report_status_accepts_known_values() {
    for (input, expected) in [
        ("done", "done"),
        ("in_review", "in_review"),
        ("blocked", "blocked"),
        ("partial", "partial"),
    ] {
        let action = parse_report(&["msg", "--status", input]).expect("status should parse");
        match action {
            AgentAction::Report(args) => {
                assert_eq!(args.status.expect("status set").as_str(), expected);
            }
            _ => panic!("expected report action"),
        }
    }
}

#[test]
fn report_status_rejects_garbage() {
    match parse_report(&["msg", "--status", "frobnicate"]) {
        Err(e) => assert_eq!(e.kind(), clap::error::ErrorKind::InvalidValue),
        Ok(_) => panic!("garbage --status must be rejected"),
    }
}

#[test]
fn report_changed_files_splits_on_comma() {
    let action = parse_report(&["msg", "--changed-files", "a.rs,b.rs,c.rs"]).unwrap();
    match action {
        AgentAction::Report(args) => {
            assert_eq!(
                args.changed_files,
                Some(vec!["a.rs".into(), "b.rs".into(), "c.rs".into()])
            );
        }
        _ => panic!("expected report action"),
    }
}

#[test]
fn explicit_report_plan_and_task_take_precedence() {
    let action =
        parse_report(&["msg", "--plan", "pl_explicit", "--task", "task_explicit"]).unwrap();
    match action {
        AgentAction::Report(args) => {
            assert_eq!(args.plan.as_deref(), Some("pl_explicit"));
            assert_eq!(args.task.as_deref(), Some("task_explicit"));
        }
        _ => panic!("expected agent report"),
    }
}

#[test]
fn report_without_flags_leaves_structured_fields_unset() {
    let action = parse_report(&["just a message"]).unwrap();
    match action {
        AgentAction::Report(args) => {
            assert!(args.status.is_none());
            assert!(args.branch.is_none());
            assert!(args.sha.is_none());
            // `--base` falls back to the dispatch-time CODETERM_LANE_BASE by design.
            assert_eq!(args.base, std::env::var("CODETERM_LANE_BASE").ok());
            assert!(args.changed_files.is_none());
            assert!(args.test_cmd.is_none());
            assert!(args.test_result.is_none());
            assert!(args.skipped.is_none());
        }
        _ => panic!("expected report action"),
    }
}

#[test]
fn agent_report_parses_full_structured_form() {
    let action = parse_report(&[
        "--status",
        "in_review",
        "--branch",
        "lane/report-cli-planning",
        "--base",
        "abc123",
        "ready",
    ])
    .expect("agent report parses");

    match action {
        AgentAction::Report(args) => {
            assert_eq!(args.message.as_deref(), Some("ready"));
            assert_eq!(args.status.expect("status set").as_str(), "in_review");
            assert_eq!(args.branch.as_deref(), Some("lane/report-cli-planning"));
            assert_eq!(args.base.as_deref(), Some("abc123"));
        }
        _ => panic!("expected agent report"),
    }
}

#[test]
fn ledger_parses_tab_and_global_json() {
    let cli = Cli::try_parse_from(["codeterm", "--json", "agent", "ledger", "--tab", "orch-1"])
        .expect("ledger should parse");
    assert!(cli.json, "global --json applies to ledger output");
    match cli.command {
        crate::Commands::Agent {
            action: AgentAction::Ledger { tab },
        } => assert_eq!(tab.as_deref(), Some("orch-1")),
        _ => panic!("expected agent ledger action"),
    }
}

#[test]
fn ack_parses_without_verbose_payload() {
    let _guard = crate::test_env_lock().lock().unwrap();
    let cli = Cli::try_parse_from(["codeterm", "agent", "ack"]).expect("ack should parse");
    match cli.command {
        crate::Commands::Agent {
            action: AgentAction::Ack { tab, plan, task },
        } => {
            assert!(tab.is_none());
            assert!(plan.is_none());
            assert!(task.is_none());
        }
        _ => panic!("expected agent ack action"),
    }
}

#[test]
fn startup_hold_parses_attempt_fence() {
    let hold = Cli::try_parse_from([
        "codeterm",
        "agent",
        "startup-hold",
        "--tab",
        "child",
        "--attempt",
        "launch-a",
    ])
    .expect("startup hold parses");
    assert!(matches!(
        hold.command,
        crate::Commands::Agent {
            action: AgentAction::StartupHold { ref tab, ref attempt }
        } if tab == "child" && attempt == "launch-a"
    ));
}

#[test]
fn startup_ack_has_no_spoofable_identity_flags() {
    let ack =
        Cli::try_parse_from(["codeterm", "agent", "startup-ack"]).expect("startup ack parses");
    assert!(matches!(
        ack.command,
        crate::Commands::Agent {
            action: AgentAction::StartupAck
        }
    ));
}

#[test]
fn ledger_tab_is_optional() {
    let cli = Cli::try_parse_from(["codeterm", "agent", "ledger"]).expect("ledger w/o tab");
    match cli.command {
        crate::Commands::Agent {
            action: AgentAction::Ledger { tab },
        } => assert!(tab.is_none()),
        _ => panic!("expected agent ledger action"),
    }
}

#[test]
fn spawn_without_provider_keeps_model_routing_available() {
    let cli = Cli::try_parse_from(["codeterm", "agent", "spawn", "--model", "sonnet"])
        .expect("provider is optional when a model can identify it");
    match cli.command {
        crate::Commands::Agent {
            action: AgentAction::Spawn { agent, model, .. },
        } => {
            assert_eq!(agent, None);
            assert_eq!(model.as_deref(), Some("sonnet"));
        }
        _ => panic!("expected agent spawn"),
    }
}

#[test]
fn spawn_planner_role_maps_to_worker_profile() {
    let cli = Cli::try_parse_from(["codeterm", "agent", "spawn", "claude", "--role", "Planner"])
        .expect("planner role parses");
    match cli.command {
        crate::Commands::Agent {
            action: AgentAction::Spawn { role, .. },
        } => {
            let role = role.expect("role set");
            assert_eq!(role.as_str(), "Manager");
            assert_eq!(role.role_profile(), Some("planner"));
        }
        _ => panic!("expected agent spawn"),
    }
}

#[test]
fn spawn_watcher_role_maps_to_manager_profile() {
    let cli = Cli::try_parse_from([
        "codeterm",
        "agent",
        "spawn",
        "fixture-backend",
        "--role",
        "Watcher",
    ])
    .expect("watcher role parses");
    match cli.command {
        crate::Commands::Agent {
            action: AgentAction::Spawn { role, .. },
        } => {
            let role = role.expect("role set");
            assert_eq!(role.as_str(), "Manager");
            assert_eq!(role.role_profile(), Some("watcher"));
        }
        _ => panic!("expected agent spawn"),
    }
}

#[test]
fn spawn_worker_plan_task_parses() {
    let cli = Cli::try_parse_from([
        "codeterm",
        "agent",
        "spawn",
        "claude",
        "--role",
        "Worker",
        "--plan",
        "pl_test",
        "--plan-task",
        "my-task",
    ])
    .expect("spawn with plan-task parses");
    match cli.command {
        crate::Commands::Agent {
            action:
                AgentAction::Spawn {
                    role,
                    plan,
                    plan_task,
                    ..
                },
        } => {
            assert!(matches!(role, Some(SpawnRole::Worker)));
            assert_eq!(plan.as_deref(), Some("pl_test"));
            assert_eq!(plan_task.as_deref(), Some("my-task"));
        }
        _ => panic!("expected agent spawn"),
    }
}

#[test]
fn agent_spawn_unplanned_parses_reason() {
    let action = parse_spawn(&[
        "codex",
        "--role",
        "Worker",
        "--unplanned",
        "--reason",
        "urgent probe",
    ])
    .expect("unplanned Worker spawn parses");
    match action {
        AgentAction::Spawn {
            role,
            unplanned,
            reason,
            ..
        } => {
            assert!(matches!(role, Some(SpawnRole::Worker)));
            assert!(unplanned);
            assert_eq!(reason.as_deref(), Some("urgent probe"));
        }
        _ => panic!("expected agent spawn"),
    }
}

/// The verification-Worker spawn: read-only, no `--plan-task`, and its
/// declared gate tokens travel as repeats of the same flag the dispatch
/// path already uses.
#[test]
fn spawn_read_only_worker_parses_without_a_plan_task() {
    let action = parse_spawn(&[
        "codex",
        "--role",
        "Worker",
        "--plan",
        "pl_test",
        "--execution-intent",
        "read_only",
        "--authorized-gate",
        "./build.sh test-one app verifier",
        "--authorized-gate",
        "./build.sh verify",
    ])
    .expect("a read-only Worker needs no plan task");
    match action {
        AgentAction::Spawn {
            role,
            plan,
            plan_task,
            execution_intent,
            authorized_gates,
            ..
        } => {
            assert!(matches!(role, Some(SpawnRole::Worker)));
            assert_eq!(plan.as_deref(), Some("pl_test"));
            assert!(plan_task.is_none());
            assert_eq!(execution_intent.as_deref(), Some("read_only"));
            assert_eq!(
                authorized_gates,
                vec![
                    "./build.sh test-one app verifier".to_string(),
                    "./build.sh verify".to_string()
                ]
            );
        }
        _ => panic!("expected agent spawn"),
    }
}

#[test]
fn taskless_worker_kind_cli_contract_needs_no_plan_task() {
    let action = parse_spawn(&[
        "codex",
        "--role",
        "Worker",
        "--kind",
        "qa",
        "--parent-tab",
        "parent_tab",
        "--authorized-gate",
        "./build.sh test-one app verifier",
    ])
    .expect("task-less qa Worker spawn parses");
    match action {
        AgentAction::Spawn {
            role,
            kind,
            parent,
            plan,
            plan_task,
            execution_intent,
            authorized_gates,
            ..
        } => {
            assert!(matches!(role, Some(SpawnRole::Worker)));
            assert_eq!(kind.as_deref(), Some("qa"));
            assert_eq!(parent.as_deref(), Some("parent_tab"));
            assert!(plan.is_none());
            assert!(plan_task.is_none());
            assert!(execution_intent.is_none());
            assert_eq!(
                authorized_gates,
                vec!["./build.sh test-one app verifier".to_string()]
            );
        }
        _ => panic!("expected agent spawn"),
    }

    let action = parse_spawn(&[
        "codex",
        "--role",
        "Worker",
        "--kind",
        "reviewer",
        "--parent-tab",
        "parent_tab",
        "--task",
        "Review the exact diff named here",
    ])
    .expect("task-less reviewer Worker spawn parses");
    match action {
        AgentAction::Spawn {
            kind,
            task,
            plan_task,
            bound_task,
            execution_intent,
            ..
        } => {
            assert_eq!(kind.as_deref(), Some("reviewer"));
            assert_eq!(task.as_deref(), Some("Review the exact diff named here"));
            assert!(plan_task.is_none());
            assert!(bound_task.is_none());
            assert!(execution_intent.is_none());
        }
        _ => panic!("expected agent spawn"),
    }
}

/// The reported defect: clap demanded `--plan-task` for any
/// `--execution-intent`, so the documented verifier could not be spawned.
#[test]
fn execution_intent_no_longer_requires_a_plan_task_at_parse_time() {
    let _guard = crate::test_env_lock().lock().unwrap();
    let action = parse_spawn(&[
        "codex",
        "--role",
        "Worker",
        "--execution-intent",
        "read_only",
    ])
    .expect("--execution-intent must parse on its own");
    match action {
        AgentAction::Spawn {
            execution_intent,
            plan,
            plan_task,
            ..
        } => {
            assert_eq!(execution_intent.as_deref(), Some("read_only"));
            assert!(plan.is_none());
            assert!(plan_task.is_none());
        }
        _ => panic!("expected agent spawn"),
    }
}

#[test]
fn spawn_provider_args_preserve_hyphen_values_after_codeterm_bindings() {
    let action = parse_spawn(&[
        "claude",
        "--role",
        "Worker",
        "--plan",
        "pl_x",
        "--task",
        "do x",
        "--args",
        "--resume",
        "1234-uuid",
        "--model",
        "gpt-5",
    ])
    .expect("provider args should parse");

    match action {
        AgentAction::Spawn {
            role,
            plan,
            task,
            args,
            model,
            ..
        } => {
            assert!(matches!(role, Some(SpawnRole::Worker)));
            assert_eq!(plan.as_deref(), Some("pl_x"));
            assert_eq!(task.as_deref(), Some("do x"));
            assert_eq!(
                args,
                Some(vec![
                    "--resume".into(),
                    "1234-uuid".into(),
                    "--model".into(),
                    "gpt-5".into(),
                ])
            );
            assert!(model.is_none());
        }
        _ => panic!("expected agent spawn"),
    }
}

#[test]
fn spawn_without_provider_args_is_unaffected() {
    let action = parse_spawn(&["claude"]).expect("spawn without options should parse");
    match action {
        AgentAction::Spawn { role, args, .. } => {
            assert!(role.is_none());
            assert!(args.is_none());
        }
        _ => panic!("expected agent spawn"),
    }

    let action = parse_spawn(&["claude", "--role", "Worker"]).expect("role should still parse");
    match action {
        AgentAction::Spawn { role, args, .. } => {
            assert!(matches!(role, Some(SpawnRole::Worker)));
            assert!(args.is_none());
        }
        _ => panic!("expected agent spawn"),
    }
}

#[test]
fn spawn_title_does_not_consume_following_flags() {
    let cli = Cli::try_parse_from([
        "codeterm",
        "agent",
        "spawn",
        "claude",
        "--title",
        "Team routing smoke",
        "--reasoning-effort",
        "low",
        "--description",
        "Team containment probe",
        "--non-user-facing",
    ])
    .expect("spawn flags after title should parse independently");
    match cli.command {
        crate::Commands::Agent {
            action:
                AgentAction::Spawn {
                    title,
                    description,
                    reasoning_effort,
                    non_user_facing,
                    ..
                },
        } => {
            assert_eq!(
                title.map(|words| words.join(" ")).as_deref(),
                Some("Team routing smoke")
            );
            assert_eq!(reasoning_effort.as_deref(), Some("low"));
            assert_eq!(
                description.map(|words| words.join(" ")).as_deref(),
                Some("Team containment probe")
            );
            assert!(non_user_facing);
        }
        _ => panic!("expected agent spawn"),
    }
}

#[test]
fn spawn_team_attaches_a_roleless_agent() {
    let action = parse_spawn(&["claude", "--team", "team_alpha"]).expect("--team parses");
    match action {
        AgentAction::Spawn {
            team,
            role,
            plan_task,
            ..
        } => {
            assert_eq!(team.as_deref(), Some("team_alpha"));
            assert!(role.is_none());
            assert!(plan_task.is_none());
        }
        _ => panic!("expected agent spawn"),
    }
}

#[test]
fn spawn_team_conflicts_with_plan_task() {
    let error = parse_spawn(&["claude", "--team", "team_alpha", "--plan-task", "task_one"])
        .expect_err("--team must not combine with a plan binding");
    assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
}

/// codeterm#418: the role that anchors a Team is the one `--role` `--team`
/// accepts, so a stranded Team can be handed a new Orchestrator. Every other
/// role is refused later, by code, not by clap.
#[test]
fn spawn_team_parses_with_an_anchoring_role() {
    let parsed = parse_spawn(&["claude", "--team", "team_alpha", "--role", "Orchestrator"])
        .expect("--team --role Orchestrator parses");
    match parsed {
        AgentAction::Spawn { team, role, .. } => {
            assert_eq!(team.as_deref(), Some("team_alpha"));
            assert!(matches!(role, Some(SpawnRole::Orchestrator)));
        }
        _ => panic!("expected agent spawn"),
    }
    assert!(
        parse_spawn(&["claude", "--team", "team_alpha", "--role", "Worker"]).is_ok(),
        "a non-anchoring role parses and is refused by the spawn verb, not by clap"
    );
}

#[test]
fn set_orchestrator_help_mentions_workers_not_leads() {
    let help = Cli::command()
        .find_subcommand_mut("agent")
        .unwrap()
        .find_subcommand_mut("set-orchestrator")
        .unwrap()
        .render_long_help()
        .to_string();
    assert!(help.contains("workers"), "{help}");
    assert!(
        help.contains("own live roleless, team-less agent"),
        "{help}"
    );
    assert!(!help.to_ascii_lowercase().contains("lead"), "{help}");
}

#[test]
fn launch_commands_accept_provider_owned_speed() {
    for args in [
        vec![
            "codeterm",
            "agent",
            "spawn",
            "fixture",
            "--speed",
            "opaque-tier",
        ],
        vec![
            "codeterm",
            "plan",
            "task",
            "dispatch",
            "plan",
            "task",
            "--execution-intent",
            "mutating",
            "--speed",
            "opaque-tier",
        ],
        vec![
            "codeterm",
            "plan",
            "task",
            "respawn",
            "plan",
            "task",
            "--attempt",
            "attempt",
            "--speed",
            "opaque-tier",
        ],
        vec![
            "codeterm",
            "plan",
            "task",
            "update",
            "plan",
            "task",
            "--speed",
            "opaque-tier",
        ],
    ] {
        Cli::try_parse_from(args).unwrap();
    }
}
