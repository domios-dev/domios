use super::*;

#[test]
fn plan_record_parses_without_plan_id() {
    let _guard = crate::test_env_lock().lock().unwrap();
    let previous = std::env::var("CODETERM_PLAN_ID").ok();
    std::env::remove_var("CODETERM_PLAN_ID");
    let parsed = Cli::try_parse_from([
        "codeterm",
        "plan",
        "record",
        "--upsert-task",
        "handoff-cli",
        "--name",
        "Handoff CLI",
        "--status",
        "in_progress",
    ]);
    match previous {
        Some(value) => std::env::set_var("CODETERM_PLAN_ID", value),
        None => std::env::remove_var("CODETERM_PLAN_ID"),
    }
    let parsed = parsed.expect("plan record parses without plan id");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Record {
                    plan_id,
                    upsert_task,
                    name,
                    sections,
                    status,
                    body_file,
                    steps,
                    links,
                    key,
                    ..
                },
        } => {
            assert_eq!(plan_id, None);
            assert_eq!(upsert_task.as_deref(), Some("handoff-cli"));
            assert_eq!(name.as_deref(), Some("Handoff CLI"));
            assert_eq!(sections, None);
            assert_eq!(status.as_deref(), Some("in_progress"));
            assert_eq!(body_file, None);
            assert!(steps.is_empty());
            assert!(links.is_empty());
            assert_eq!(key, None);
        }
        _ => panic!("expected plan record"),
    }
}

#[test]
fn plan_state_allows_missing_plan_id() {
    let parsed = Cli::try_parse_from(["codeterm", "plan", "state"])
        .expect("plan state parses without plan id");
    match parsed.command {
        Commands::Plan {
            action: PlanAction::State { plan_id, .. },
        } => assert_eq!(plan_id, None),
        _ => panic!("expected plan state"),
    }
}

#[test]
fn plan_task_list_allows_missing_plan_id() {
    let parsed = Cli::try_parse_from(["codeterm", "plan", "task", "list", "--status", "blocked"])
        .expect("plan task list parses without plan id");
    match parsed.command {
        Commands::Plan {
            action:
                PlanAction::Task {
                    action:
                        crate::args::plan::PlanTaskAction::List {
                            plan_id,
                            status,
                            all,
                        },
                },
        } => {
            assert_eq!(plan_id, None);
            assert_eq!(status.as_deref(), Some("blocked"));
            assert!(!all);
        }
        _ => panic!("expected plan task list"),
    }
}

#[test]
fn plan_get_allows_missing_plan_id() {
    let parsed = Cli::try_parse_from(["codeterm", "plan", "get", "--sections"])
        .expect("plan get parses without plan id");
    match parsed.command {
        Commands::Plan {
            action: PlanAction::Get { plan_id, sections },
        } => {
            assert_eq!(plan_id, None);
            assert!(sections);
        }
        _ => panic!("expected plan get"),
    }
}

#[test]
fn state_and_next_detail_windows_require_valid_explicit_pages() {
    for command in ["state", "next", "lanes"] {
        assert!(Cli::try_parse_from(["domios", "plan", command, "plan", "--page", "2"]).is_ok());
        assert!(Cli::try_parse_from(["domios", "plan", command, "plan", "--all"]).is_ok());
        assert!(Cli::try_parse_from(["domios", "plan", command, "plan", "--page", "0"]).is_err());
        assert!(
            Cli::try_parse_from(["domios", "plan", command, "plan", "--page", "2", "--all"])
                .is_err()
        );
    }
}
