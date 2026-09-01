use super::*;
use crate::client::SocketCallError;

#[derive(Default)]
struct FakeRpc {
    calls: Vec<(String, Option<Value>)>,
}

impl RpcClient for FakeRpc {
    fn call_detailed(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, SocketCallError> {
        self.calls.push((method.to_string(), params));
        Ok(match method {
            "plan.skills.list" => json!({ "plan_id": "plan_rpc", "skills": [] }),
            "plan.skills.show" => json!({
                "plan_id": "plan_rpc",
                "skill": { "slug": "deploy", "body": "", "ledger": [] }
            }),
            "plan.skills.new" => json!({
                "plan_id": "plan_rpc",
                "skill": {
                    "slug": "deploy",
                    "skill_md": "remote/SKILL.md",
                    "scripts": "remote/scripts",
                    "ledger_seq": 1,
                    "verdict": "keep"
                }
            }),
            "plan.files" => json!({
                "index_path": "C:/Users/dev/.codeterm/plans/active/plan_rpc/INDEX.md",
                "entries": []
            }),
            other => panic!("unexpected RPC method {other}"),
        })
    }
}

/// codeterm-releases#53: `plan skill list` prints each skill's on-disk path,
/// a fallback for a caller whose `show` is refused.
#[test]
fn plan_skill_list_prints_skill_path() {
    let response = json!({
        "plan_id": "pl_zzmxgb",
        "skills": [
            {
                "slug": "verify-before-claim",
                "verdict": "keep",
                "skill_dir": "/repo/.codeterm/plans/active/pl_zzmxgb/skills/verify-before-claim",
            }
        ]
    });

    let rendered = render_skill_list(&response).unwrap();

    assert!(rendered.contains(
        "/repo/.codeterm/plans/active/pl_zzmxgb/skills/verify-before-claim"
    ));
    assert!(rendered.contains("verify-before-claim"));
}

#[test]
fn local_skill_edit_resolves_authoritative_plan_directory_from_rpc() {
    let mut client = FakeRpc::default();

    let (plan_id, plan_dir) = resolve_local(&mut client, Some("plan_rpc".into())).unwrap();

    assert_eq!(plan_id, "plan_rpc");
    assert!(plan_dir.ends_with("active/plan_rpc"));
    assert_eq!(client.calls[0].0, "plan.files");
    assert_eq!(client.calls[0].1.as_ref().unwrap()["plan_id"], "plan_rpc");
}

#[test]
fn list_show_and_new_route_explicit_plan_to_rpc() {
    let _guard = crate::test_env_lock().lock().unwrap();
    let worker_keys = [
        "CODETERM_TAB_ID",
        "CODETERM_SESSION_ID",
        "CODETERM_SESSION_BINDING_NONCE",
        "CODETERM_TASK_ID",
        "CODETERM_ATTEMPT_ID",
    ];
    let previous = worker_keys.map(|key| (key, std::env::var(key).ok()));
    for key in worker_keys {
        std::env::remove_var(key);
    }
    let mut client = FakeRpc::default();
    handle(
        &mut client,
        true,
        PlanSkillAction::List {
            plan: Some("plan_explicit".into()),
        },
    )
    .unwrap();
    handle(
        &mut client,
        true,
        PlanSkillAction::Show {
            slug: "deploy".into(),
            plan: Some("plan_explicit".into()),
        },
    )
    .unwrap();
    handle(
        &mut client,
        true,
        PlanSkillAction::New {
            slug: "Deploy Safely".into(),
            plan: Some("plan_explicit".into()),
            title: Some("Safe Deploy".into()),
            area: Some("release".into()),
            check: Some("./scripts/check.sh".into()),
            rationale: Some("accepted".into()),
        },
    )
    .unwrap();

    assert_eq!(
        client.calls,
        vec![
            (
                "plan.skills.list".into(),
                Some(json!({ "plan_id": "plan_explicit" }))
            ),
            (
                "plan.skills.show".into(),
                Some(json!({ "plan_id": "plan_explicit", "slug": "deploy" }))
            ),
            (
                "plan.skills.new".into(),
                Some(json!({
                    "plan_id": "plan_explicit",
                    "slug": "Deploy Safely",
                    "title": "Safe Deploy",
                    "area": "release",
                    "check": "./scripts/check.sh",
                    "rationale": "accepted"
                }))
            )
        ]
    );
    for (key, value) in previous {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}

#[test]
fn list_show_and_new_use_plan_id_from_environment() {
    let _guard = crate::test_env_lock().lock().unwrap();
    // The gate runs inside a live Orchestrator, so an inherited partial worker
    // binding would be refused before the RPC this test is about.
    let cleared = [
        "CODETERM_TAB_ID",
        "CODETERM_SESSION_ID",
        "CODETERM_SESSION_BINDING_NONCE",
        "CODETERM_TASK_ID",
        "CODETERM_ATTEMPT_ID",
    ];
    let restore = cleared.map(|key| (key, std::env::var(key).ok()));
    for key in cleared {
        std::env::remove_var(key);
    }
    let previous = std::env::var("CODETERM_PLAN_ID").ok();
    std::env::set_var("CODETERM_PLAN_ID", "plan_environment");
    let mut client = FakeRpc::default();

    handle(&mut client, true, PlanSkillAction::List { plan: None }).unwrap();
    handle(
        &mut client,
        true,
        PlanSkillAction::Show {
            slug: "deploy".into(),
            plan: None,
        },
    )
    .unwrap();
    handle(
        &mut client,
        true,
        PlanSkillAction::New {
            slug: "deploy".into(),
            plan: None,
            title: None,
            area: None,
            check: None,
            rationale: None,
        },
    )
    .unwrap();

    for (_, params) in &client.calls {
        assert_eq!(
            params.as_ref().unwrap()["plan_id"],
            serde_json::json!("plan_environment")
        );
    }
    match previous {
        Some(value) => std::env::set_var("CODETERM_PLAN_ID", value),
        None => std::env::remove_var("CODETERM_PLAN_ID"),
    }
    for (key, value) in restore {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}

/// Production exports tab/nonce/task/attempt before PTY spawn, never a session.
#[test]
fn plan_skill_show_omits_unproduced_session_id() {
    let _guard = crate::test_env_lock().lock().unwrap();
    let keys = [
        "CODETERM_TAB_ID",
        "CODETERM_SESSION_ID",
        "CODETERM_SESSION_BINDING_NONCE",
        "CODETERM_TASK_ID",
        "CODETERM_ATTEMPT_ID",
    ];
    let previous = keys.map(|key| (key, std::env::var(key).ok()));
    for key in keys {
        std::env::remove_var(key);
    }
    for (key, value) in [
        ("CODETERM_TAB_ID", "ced263e9"),
        ("CODETERM_SESSION_BINDING_NONCE", "nonce-5upl98"),
        ("CODETERM_TASK_ID", "task_6bd5ky"),
        ("CODETERM_ATTEMPT_ID", "attempt_5upl98"),
    ] {
        std::env::set_var(key, value);
    }
    let mut client = FakeRpc::default();
    handle(
        &mut client,
        true,
        PlanSkillAction::Show {
            slug: "dispatch-contract".into(),
            plan: Some("pl_zzmxgb".into()),
        },
    )
    .unwrap();
    let params = client.calls[0].1.as_ref().unwrap();
    assert_eq!(params["caller_tab_id"], "ced263e9");
    assert_eq!(params["session_binding_nonce"], "nonce-5upl98");
    assert_eq!(params["plan_id"], "pl_zzmxgb");
    assert_eq!(params["task_id"], "task_6bd5ky");
    assert_eq!(params["attempt_id"], "attempt_5upl98");
    assert!(params.get("caller_session_id").is_none());
    for (key, value) in previous {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}

/// A task assigned after launch leaves no task/attempt env; tab+nonce still route.
#[test]
fn plan_skill_show_resolves_post_launch_task_from_live_tab_binding() {
    let _guard = crate::test_env_lock().lock().unwrap();
    let keys = [
        "CODETERM_TAB_ID",
        "CODETERM_SESSION_ID",
        "CODETERM_SESSION_BINDING_NONCE",
        "CODETERM_TASK_ID",
        "CODETERM_ATTEMPT_ID",
    ];
    let previous = keys.map(|key| (key, std::env::var(key).ok()));
    for key in keys {
        std::env::remove_var(key);
    }
    std::env::set_var("CODETERM_TAB_ID", "ced263e9");
    std::env::set_var("CODETERM_SESSION_BINDING_NONCE", "nonce-5upl98");
    let mut client = FakeRpc::default();
    handle(
        &mut client,
        true,
        PlanSkillAction::Show {
            slug: "dispatch-contract".into(),
            plan: Some("pl_zzmxgb".into()),
        },
    )
    .unwrap();
    let params = client.calls[0].1.as_ref().unwrap();
    assert_eq!(params["caller_tab_id"], "ced263e9");
    assert_eq!(params["session_binding_nonce"], "nonce-5upl98");
    assert!(params.get("caller_session_id").is_none());
    assert!(params.get("task_id").is_none());
    assert!(params.get("attempt_id").is_none());
    for (key, value) in previous {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}

/// A caller claiming any single identity fragment without the required
/// tab+nonce pair is refused before the RPC — never silently dropped.
#[test]
fn plan_skill_show_rejects_partial_live_binding_before_rpc() {
    let _guard = crate::test_env_lock().lock().unwrap();
    let keys = [
        "CODETERM_TAB_ID",
        "CODETERM_SESSION_ID",
        "CODETERM_SESSION_BINDING_NONCE",
        "CODETERM_TASK_ID",
        "CODETERM_ATTEMPT_ID",
    ];
    let previous = keys.map(|key| (key, std::env::var(key).ok()));
    for partial in [
        ("CODETERM_TASK_ID", "task_6bd5ky"),
        ("CODETERM_ATTEMPT_ID", "attempt_5upl98"),
        ("CODETERM_TAB_ID", "ced263e9"),
        ("CODETERM_SESSION_BINDING_NONCE", "nonce-5upl98"),
        ("CODETERM_SESSION_ID", "session-5upl98"),
    ] {
        for key in keys {
            std::env::remove_var(key);
        }
        std::env::set_var(partial.0, partial.1);
        let mut client = FakeRpc::default();
        let error = handle(
            &mut client,
            true,
            PlanSkillAction::Show {
                slug: "dispatch-contract".into(),
                plan: Some("pl_zzmxgb".into()),
            },
        )
        .unwrap_err();
        assert!(!error.is_empty(), "{} alone must be refused", partial.0);
        assert!(client.calls.is_empty());
    }
    for (key, value) in previous {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}

/// codeterm-releases#53: a refused `plan skill show` must return `Err`,
/// since the top-level CLI runner maps every `Err` to a non-zero exit.
#[test]
fn plan_skill_show_refusal_exits_nonzero() {
    let _guard = crate::test_env_lock().lock().unwrap();
    let keys = [
        "CODETERM_TAB_ID",
        "CODETERM_SESSION_ID",
        "CODETERM_SESSION_BINDING_NONCE",
        "CODETERM_TASK_ID",
        "CODETERM_ATTEMPT_ID",
    ];
    let previous = keys.map(|key| (key, std::env::var(key).ok()));
    for key in keys {
        std::env::remove_var(key);
    }
    // A task-id claim with no tab id is refused before any RPC is sent.
    std::env::set_var("CODETERM_TASK_ID", "task_6bd5ky");
    let mut client = FakeRpc::default();

    let result = handle(
        &mut client,
        true,
        PlanSkillAction::Show {
            slug: "dispatch-contract".into(),
            plan: Some("pl_zzmxgb".into()),
        },
    );

    assert!(result.is_err(), "a refused show must be an Err, not a printed message with Ok");
    assert!(client.calls.is_empty(), "the client-side gate must not reach the RPC");

    for (key, value) in previous {
        match value {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}
