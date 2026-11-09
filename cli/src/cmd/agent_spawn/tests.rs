use super::*;
use crate::cmd::hints::unbound_worker_binding_hint;
use crate::cmd::report_guard::worker_binding_resolved;

#[test]
fn spawned_profile_model_uses_its_declared_name() {
    let model = r#"["gateway","opaque-model"]"#;
    let result = serde_json::json!({"agent":"fixture","model":model});
    let catalogue = serde_json::json!([
        {"id":"other","configOptions":[{"category":"model","options":[{"value":model,"name":"Wrong profile"}]}]},
        {"id":"chosen","configOptions":[{"category":"model","options":[{"value":model,"name":"Declared model"}]}]}
    ]);
    assert_eq!(
        spawn_model_label(&result, &catalogue, Some("chosen")),
        "fixture/Declared model"
    );
    assert_eq!(
        spawn_model_label(&result, &catalogue, Some("missing")),
        format!("fixture/{model}")
    );
    assert_eq!(
        spawn_model_label(
            &serde_json::json!({"agent":"fixture"}),
            &catalogue,
            Some("chosen")
        ),
        "fixture"
    );
}

fn worker(execution_intent: Option<&'static str>, has_plan: bool) -> SpawnContext<'static> {
    SpawnContext {
        role: Some(&SpawnRole::Worker),
        has_plan,
        execution_intent,
        ..Default::default()
    }
}

#[test]
fn worker_kinds_default_to_mutating_and_support_task_bindings() {
    assert_eq!(
        resolved_execution_intent(Some("qa"), None).as_deref(),
        Some(MUTATING)
    );
    assert_eq!(
        resolved_execution_intent(Some("release_manager"), None).as_deref(),
        Some(MUTATING)
    );
    for kind in ["qa", "release_manager", "reviewer"] {
        for has_plan_task in [false, true] {
            assert!(
                validate_spawn_context(&SpawnContext {
                    role: Some(&SpawnRole::Worker),
                    kind: Some(kind),
                    has_parent: true,
                    has_plan: true,
                    has_plan_task,
                    execution_intent: Some(MUTATING),
                    ..Default::default()
                })
                .is_ok(),
                "{kind} task={has_plan_task}"
            );
        }
        assert!(
            validate_spawn_context(&SpawnContext {
                role: Some(&SpawnRole::Worker),
                kind: Some(kind),
                has_parent: true,
                has_authorized_gates: true,
                execution_intent: Some(READ_ONLY),
                ..Default::default()
            })
            .is_ok(),
            "{kind}"
        );
        assert!(
            validate_spawn_context(&SpawnContext {
                role: Some(&SpawnRole::Worker),
                kind: Some(kind),
                execution_intent: Some(READ_ONLY),
                ..Default::default()
            })
            .is_err(),
            "{kind} without parent"
        );
        assert!(
            validate_spawn_context(&SpawnContext {
                role: Some(&SpawnRole::Worker),
                kind: Some(kind),
                has_parent: true,
                has_plan_task: true,
                execution_intent: Some(READ_ONLY),
                ..Default::default()
            })
            .is_ok(),
            "{kind} with task"
        );
    }
}

#[test]
fn reviewer_kind_defaults_to_a_read_only_taskless_review() {
    assert_eq!(
        resolved_execution_intent(Some("reviewer"), None).as_deref(),
        Some(READ_ONLY)
    );
    assert_eq!(
        resolved_execution_intent(Some("reviewer"), Some(MUTATING.into())).as_deref(),
        Some(MUTATING),
        "only the parent's explicit intent makes a reviewer mutate"
    );
    assert_eq!(
        resolved_execution_intent(Some("code-reviewer"), None).as_deref(),
        Some(MUTATING)
    );
    let reviewer = SpawnContext {
        role: Some(&SpawnRole::Worker),
        kind: Some("reviewer"),
        has_parent: true,
        execution_intent: Some(READ_ONLY),
        ..Default::default()
    };
    assert!(validate_spawn_context(&reviewer).is_ok());
    assert!(reviewer.is_verification_worker());
}

/// codeterm#305: an unbound Worker spawn is accepted (prod-docs/SKILLS.md's
/// ad-hoc pattern) but carries the shared advisory constant.
#[test]
fn worker_without_plan_binding_is_refused_or_warned_at_spawn() {
    assert!(validate_spawn_context(&worker(None, false)).is_ok());
    let bound = worker_binding_resolved(false, false, false);
    assert!(!bound);
    assert!(unbound_worker_binding_hint(true, bound, false).is_some());
}

#[test]
fn plan_bound_worker_requires_an_exact_task_identity() {
    assert!(validate_spawn_context(&worker(None, true)).is_err());
    assert!(validate_spawn_context(&SpawnContext {
        has_bound_task: true,
        ..worker(None, true)
    })
    .is_ok());
    assert!(validate_spawn_context(&SpawnContext {
        has_plan_task: true,
        ..worker(None, true)
    })
    .is_ok());
    assert!(validate_spawn_context(&worker(None, false)).is_ok());
    assert!(validate_spawn_context(&SpawnContext {
        has_plan: true,
        ..Default::default()
    })
    .is_ok());
}

/// The verification Worker: read-only, owns no task, and is accepted with
/// or without the plan it was spawned under.
#[test]
fn a_read_only_worker_is_accepted_without_any_task_binding() {
    for has_plan in [false, true] {
        let context = worker(Some(READ_ONLY), has_plan);
        assert!(
            validate_spawn_context(&context).is_ok(),
            "read-only Worker with has_plan={has_plan} must be accepted"
        );
        assert!(context.is_verification_worker());
    }
}

/// The safeguard that stays: source-changing work still owns a task.
#[test]
fn a_mutating_worker_without_a_task_is_refused_and_names_both_shapes() {
    for has_plan in [false, true] {
        let error = validate_spawn_context(&worker(Some(MUTATING), has_plan))
            .expect_err("a mutating Worker owns a task");
        assert!(error.contains("--plan-task"), "{error}");
        assert!(error.contains("--execution-intent read_only"), "{error}");
    }
    assert!(validate_spawn_context(&SpawnContext {
        has_plan_task: true,
        ..worker(Some(MUTATING), true)
    })
    .is_ok());
}

/// A refusal that only demanded the flag left the other supported shape
/// invisible, which is how a verifier had nowhere to be spawned from.
#[test]
fn the_plan_bound_worker_refusal_names_the_read_only_shape() {
    let error = validate_spawn_context(&worker(None, true))
        .expect_err("--plan without a task binding is refused");
    assert!(error.contains("--execution-intent read_only"), "{error}");
    assert!(error.contains("--authorized-gate"), "{error}");
    assert!(error.contains("--plan-task"), "{error}");
}

#[test]
fn declared_gates_belong_to_a_task_less_worker_only() {
    assert!(validate_spawn_context(&SpawnContext {
        has_authorized_gates: true,
        ..worker(Some(READ_ONLY), true)
    })
    .is_ok());

    let roleless = validate_spawn_context(&SpawnContext {
        has_authorized_gates: true,
        ..Default::default()
    })
    .expect_err("gates are a Worker concept");
    assert!(roleless.contains("--role Worker"), "{roleless}");

    let task_bound = validate_spawn_context(&SpawnContext {
        has_authorized_gates: true,
        has_plan_task: true,
        ..worker(Some(READ_ONLY), true)
    })
    .expect_err("a task-bound spawn declares its gates on the dispatch");
    assert!(task_bound.contains("plan task dispatch"), "{task_bound}");
}

#[test]
fn team_spawn_fields_require_a_caller_tab() {
    let (team_id, caller) = team_spawn_fields(" team_alpha ", Some(" tab_manager ")).unwrap();
    assert_eq!(team_id, "team_alpha");
    assert_eq!(caller, "tab_manager");

    for caller in [None, Some(""), Some("   ")] {
        let error = team_spawn_fields("team_alpha", caller).unwrap_err();
        assert!(error.contains("CODETERM_TAB_ID"), "{error}");
    }
}

#[test]
fn team_spawn_fields_reject_an_empty_team() {
    assert!(team_spawn_fields("  ", Some("tab_manager"))
        .unwrap_err()
        .contains("Team id"));
}

/// codeterm#418: `--team --role Orchestrator` is the recovery add and needs
/// no `--workspace`; the Team names the workspace.
#[test]
fn team_supplies_the_workspace_for_an_orchestrator_spawn() {
    assert!(validate_spawn_context(&SpawnContext {
        role: Some(&SpawnRole::Orchestrator),
        has_team: true,
        ..Default::default()
    })
    .is_ok());
    assert!(validate_spawn_context(&SpawnContext {
        role: Some(&SpawnRole::Orchestrator),
        ..Default::default()
    })
    .is_err());
}

#[test]
fn team_spawn_accepts_only_the_anchoring_role() {
    assert!(validate_team_spawn_role(None).is_ok());
    assert!(validate_team_spawn_role(Some(&SpawnRole::Orchestrator)).is_ok());
    for role in [
        SpawnRole::Worker,
        SpawnRole::Planner,
        SpawnRole::Manager,
        SpawnRole::Watcher,
    ] {
        let error = validate_team_spawn_role(Some(&role))
            .expect_err("a non-anchoring role takes its Team from its own binding");
        assert!(
            error.starts_with(team_codes::TEAM_SPAWN_ROLE_MUST_ANCHOR),
            "the refusal must lead with its code, got: {error}"
        );
        if matches!(role, SpawnRole::Planner) {
            assert!(error.contains("got Planner"));
        }
    }
}

/// The CLI recovery add leaves `agent.spawn` and lands on the same daemon
/// staffing verb the UI add-member action uses.
#[test]
fn only_an_anchoring_role_with_a_team_routes_to_team_staffing() {
    assert!(routes_to_team_staffing(
        Some(&SpawnRole::Orchestrator),
        Some("team_alpha")
    ));
    assert!(!routes_to_team_staffing(
        Some(&SpawnRole::Orchestrator),
        None
    ));
    assert!(!routes_to_team_staffing(None, Some("team_alpha")));
    assert!(!routes_to_team_staffing(
        Some(&SpawnRole::Worker),
        Some("team_alpha")
    ));
}

/// The provisioning Worker staffed on a mesh peer: it changes source, but
/// `plan task dispatch` is local-only, so no binding exists for it to own.
/// That shape is admitted, and the mutating declaration — the one case
/// where a binding is possible and therefore required — still is not.
#[test]
fn a_mesh_worker_owns_no_task_and_is_still_admitted() {
    validate_spawn_context(&worker(None, false))
        .expect("a Worker with no plan and no declared intent is a supported shape");

    let refused = validate_spawn_context(&worker(Some(MUTATING), false))
        .expect_err("declaring mutating intent asks for a binding");
    assert!(refused.contains("--plan-task"), "{refused}");
}

#[test]
fn unsupported_recovery_flags_are_named_not_dropped() {
    assert!(unsupported_team_orchestrator_flags(&[("--cwd", false)]).is_empty());
    assert_eq!(
        unsupported_team_orchestrator_flags(&[
            ("--cwd", true),
            ("--preset", false),
            ("--workspace", true),
        ]),
        vec!["--cwd", "--workspace"]
    );
}
#[test]
fn unplanned_dispatch_preserves_explicit_foreign_repository_cwd() {
    let caller = tempfile::tempdir().unwrap();
    let foreign = tempfile::tempdir().unwrap();
    for cwd in [
        foreign.path().to_path_buf(),
        std::path::PathBuf::from("../foreign-worktree"),
    ] {
        let params = super::unplanned_dispatch_params(
            Some("opaque-provider".into()),
            "opaque-reason".into(),
            "opaque-brief".into(),
            vec![],
            Some(cwd.to_str().unwrap()),
            Some(caller.path()),
        );
        let expected = if cwd.is_absolute() {
            cwd
        } else {
            caller.path().join(cwd)
        };
        assert_eq!(params["cwd"], expected.to_string_lossy().as_ref());
    }
    let inherited =
        super::unplanned_dispatch_params(None, "reason".into(), String::new(), vec![], None, None);
    assert!(inherited.get("cwd").is_none());
}
