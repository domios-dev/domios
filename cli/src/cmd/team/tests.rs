use super::*;

#[test]
fn adopt_plan_params_require_nonempty_caller() {
    let params = adopt_plan_params("team".into(), "plan".into(), Some("manager")).unwrap();
    assert_eq!(params["team_id"], "team");
    assert_eq!(params["plan_id"], "plan");
    assert_eq!(params["caller_tab_id"], "manager");

    for caller in [None, Some(""), Some("  ")] {
        let error = adopt_plan_params("team".into(), "plan".into(), caller).unwrap_err();
        assert!(error.contains("CODETERM_TAB_ID"));
    }
}

#[test]
fn adopt_agent_params_require_nonempty_caller() {
    let params = adopt_agent_params("team".into(), "tab_seven".into(), Some("manager")).unwrap();
    assert_eq!(params["team_id"], "team");
    assert_eq!(params["tab_id"], "tab_seven");
    assert_eq!(params["caller_tab_id"], "manager");

    for caller in [None, Some(""), Some("  ")] {
        let error = adopt_agent_params("team".into(), "tab_seven".into(), caller).unwrap_err();
        assert!(error.contains("CODETERM_TAB_ID"));
    }
}

#[test]
fn cartographer_params_use_the_shared_team_member_contract() {
    assert_eq!(
        cartographer_params(
            "team_a".into(),
            "provider-neutral".into(),
            Some("model-1".into()),
            Some("manager-tab"),
            Some("opaque-owner-brief\n`$()`".into()),
        ),
        json!({
            "team_id": "team_a",
            "provider": "provider-neutral",
            "role": "cartographer",
            "model": "model-1",
            "caller_tab_id": "manager-tab",
            "brief": "opaque-owner-brief\n`$()`",
        })
    );
}

#[test]
fn cartographer_brief_stdin_is_opaque_and_optional() {
    let token = "opaque-owner-brief\n`$()`\n</brief>";
    assert_eq!(
        resolve_cartographer_brief(Some("-".into()), &mut std::io::Cursor::new(token))
            .unwrap()
            .as_deref(),
        Some(token)
    );
    let mut untouched = std::io::Cursor::new(token);
    assert_eq!(
        resolve_cartographer_brief(None, &mut untouched).unwrap(),
        None
    );
    assert_eq!(untouched.position(), 0);
}

#[test]
fn close_team_params_echo_the_target_for_headerless_human_confirmation() {
    assert_eq!(
        close_team_params("team-1", None, false),
        json!({
            "team_id": "team-1",
            "force": true,
            "confirm": "team-1",
            "caller_tab_id": null,
            "force_busy": false,
        })
    );
    assert_eq!(
        close_team_params("team-1", Some("manager-1"), false)["caller_tab_id"],
        "manager-1",
    );
    assert_eq!(close_team_params("team-1", None, true)["force_busy"], true);
}

#[test]
fn forced_close_lines_name_every_killed_worker_task_and_flagged_plan() {
    let lines = forced_close_lines(&json!({
        "team": { "id": "team-1" },
        "killed_workers": [
            { "tab_id": "tab-w1", "title": "Ranker", "working": true, "task_ids": ["task-1"] },
            { "tab_id": "tab-w2", "title": "Idle", "working": true, "task_ids": [] },
        ],
        "unfinished_tasks": [
            { "plan_id": "pl-1", "task_id": "task-1", "name": "Rank", "tab_id": "tab-w1" },
        ],
        "flagged_plans": [ { "plan_id": "pl-1", "reason": "reason-token" } ],
    }));
    assert_eq!(lines.len(), 4);
    assert!(lines[0].contains("tab-w1") && lines[0].contains("task-1"));
    assert!(lines[1].contains("tab-w2"));
    assert!(lines[2].contains("task-1") && lines[2].contains("pl-1"));
    assert!(lines[3].contains("pl-1") && lines[3].contains("reason-token"));
    assert!(forced_close_lines(&json!({ "team": { "id": "team-1" } })).is_empty());
}

#[test]
fn cartographer_attachment_summary_distinguishes_reuse_from_creation() {
    let base = json!({
        "tab_id": "cartographer-tab",
        "team": { "id": "team_a" },
        "reused": false,
    });
    assert_eq!(
        cartographer_attachment_summary(&base).as_deref(),
        Some("Attached Cartographer Manager as Tab cartographer-tab in Team team_a.")
    );
    assert_eq!(
        cartographer_attachment_summary(
            &json!({ "reused": true, "tab_id": "cartographer-tab", "team": { "id": "team_a" } })
        )
        .as_deref(),
        Some("Reused existing Cartographer Manager as Tab cartographer-tab in Team team_a.")
    );
}

#[test]
fn team_show_labels_cartographer_as_an_attached_manager() {
    assert_eq!(
        team_member_role_label(&json!({
            "role": "cartographer",
            "role_type": "Manager",
            "role_profile": "cartographer",
        })),
        "Manager (Cartographer)"
    );
    assert_eq!(
        team_member_role_label(&json!({ "role": "planner" })),
        "planner"
    );
}

#[test]
fn general_agent_team_guard_refusal_reaches_the_cli_unedited() {
    // Opaque on purpose: the CLI must forward whatever the daemon says, so
    // a reworded refusal here would hide the violation id and the fix.
    let refusal = "some.violation.id: Tab tab_x is refused. Do this instead:\n  \
                       domios team pair --orchestrator alpha --planner beta --workspace ws_7";
    let rendered = render_daemon_error(&SocketCallError::Transport(refusal.into()));
    assert!(rendered.ends_with(refusal), "{rendered}");
    assert_eq!(rendered.len(), refusal.len() + "Error: ".len());
}

#[test]
fn general_agent_team_guard_pair_params_carry_the_workspace_the_refusal_teaches() {
    let params = pair_params("alpha", "beta", Some("ws_7".into()), None, false);
    assert_eq!(params["orchestrator_agent"], "alpha");
    assert_eq!(params["planner_agent"], "beta");
    assert_eq!(params["workspace_id"], "ws_7");
}

/// codeterm#400: the CLI must not silently declare leaf intent — the flag
/// carries through to the daemon only when the caller actually passed it.
#[test]
fn pair_params_omit_leaf_declaration_unless_the_flag_was_passed() {
    let params = pair_params("alpha", "beta", None, None, false);
    assert!(params.get("leaf_creation_acknowledged").is_none());

    let params = pair_params("alpha", "beta", None, None, true);
    assert_eq!(params["leaf_creation_acknowledged"], true);
}

#[test]
fn pair_params_preserve_requested_team_title() {
    let params = pair_params("alpha", "beta", None, Some("Research Team".into()), false);
    assert_eq!(params["title"], "Research Team");
}

#[test]
fn splits_provider_and_model() {
    assert_eq!(
        split_provider_model("claude:fable"),
        ("claude".to_string(), Some("fable".to_string()))
    );
}

#[test]
fn provider_only_has_no_model() {
    assert_eq!(split_provider_model("codex"), ("codex".to_string(), None));
}

#[test]
fn trailing_colon_with_no_model_falls_back_to_provider_only() {
    assert_eq!(
        split_provider_model("claude:"),
        ("claude".to_string(), None)
    );
}
