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
}

#[test]
fn list_show_and_new_use_plan_id_from_environment() {
    let _guard = crate::test_env_lock().lock().unwrap();
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
}
