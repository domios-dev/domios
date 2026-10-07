use super::*;
use crate::args::plan::PlanReviewAction;
use crate::client::{RpcClient, SocketCallError};

struct CallerEnv {
    saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

impl CallerEnv {
    fn set(tab: Option<&str>, session: Option<&str>, nonce: Option<&str>) -> Self {
        let fields = [
            ("CODETERM_TAB_ID", tab),
            ("CODETERM_SESSION_ID", session),
            ("CODETERM_SESSION_BINDING_NONCE", nonce),
        ];
        let saved = fields
            .iter()
            .map(|(key, _)| (*key, std::env::var_os(key)))
            .collect();
        for (key, value) in fields {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
        Self { saved }
    }
}

impl Drop for CallerEnv {
    fn drop(&mut self) {
        for (key, value) in &self.saved {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

#[derive(Default)]
struct Recorder {
    calls: Vec<(String, Value)>,
}

impl RpcClient for Recorder {
    fn call_detailed(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, SocketCallError> {
        self.calls.push((method.into(), params.unwrap()));
        Ok(json!({ "ok": true }))
    }
}

struct AttemptRebindRecorder {
    calls: Vec<(String, Value)>,
    response: Value,
    fail_rebind: bool,
}

impl RpcClient for AttemptRebindRecorder {
    fn call_detailed(
        &mut self,
        method: &str,
        params: Option<Value>,
    ) -> Result<Value, SocketCallError> {
        let params = params.unwrap_or(Value::Null);
        self.calls.push((method.into(), params));
        match method {
            "task.show" => Ok(json!({
                "plan_id": "pl_exact",
                "task": {
                    "id": "task_exact",
                    "interruption": { "owner_epoch": 17 },
                }
            })),
            "plan.session.rebind" if self.fail_rebind => {
                Err(SocketCallError::Rpc(crate::client::RpcError {
                    code: -32003,
                    message: "restoration refused".into(),
                    data: Some(json!({ "type": "attempt_binding_restore_refused" })),
                }))
            }
            "plan.session.rebind" => Ok(self.response.clone()),
            _ => Err(SocketCallError::Transport(format!(
                "unexpected RPC {method}"
            ))),
        }
    }
}

fn targeted_rebind_action(plan_id: Option<&str>) -> PlanAction {
    PlanAction::Rebind {
        plan_id: plan_id.map(str::to_string),
        task_id: Some("COD-1".into()),
        attempt_id: Some("attempt_exact".into()),
    }
}

fn lifecycle_actions() -> Vec<(&'static str, PlanAction)> {
    vec![
        (
            "plan.resume",
            PlanAction::Resume {
                plan_id: "pl_recovered".into(),
            },
        ),
        (
            "plan.update",
            PlanAction::Update {
                plan_id: "pl_recovered".into(),
                title: None,
                spec: None,
                spec_file: None,
                status: None,
                mode: None,
                planner: Some("none".into()),
                note: Some("bounded repair".into()),
            },
        ),
        (
            "plan.pause",
            PlanAction::Pause {
                plan_id: "pl_recovered".into(),
                reason: "owner decision".into(),
            },
        ),
        (
            "plan.unarchive",
            PlanAction::Unarchive {
                plan_id: "pl_recovered".into(),
            },
        ),
        (
            "plan.disown",
            PlanAction::Disown {
                plan_id: "pl_recovered".into(),
            },
        ),
        (
            "plan.claim",
            PlanAction::Claim {
                plan_id: "pl_recovered".into(),
            },
        ),
    ]
}

#[test]
fn lifecycle_rpcs_forward_the_callers_binding_with_optional_session() {
    let _lock = crate::test_env_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    for (tab, nonce) in [
        ("recovered-owner", "owner-nonce"),
        ("foreign-tab", "foreign-nonce"),
    ] {
        for session in [None, Some("caller-session")] {
            let _env = CallerEnv::set(Some(tab), session, Some(nonce));
            for (method, action) in lifecycle_actions() {
                let mut recorder = Recorder::default();
                run_plan(&mut recorder, true, action).unwrap();
                assert_eq!(recorder.calls.len(), 1);
                let (actual_method, params) = &recorder.calls[0];
                assert_eq!(actual_method, method);
                assert_eq!(params["plan_id"], "pl_recovered");
                assert_eq!(params["caller_tab_id"], tab);
                assert_eq!(params["session_binding_nonce"], nonce);
                assert_eq!(
                    params.get("session_id"),
                    session.map(|value| json!(value)).as_ref()
                );
                assert!(params.get("human_confirmation").is_none());
                if method == "plan.pause" {
                    assert_eq!(params["reason"], "owner decision");
                    assert_eq!(params["from"]["tab_id"], tab);
                }
                if method == "plan.claim" {
                    assert_eq!(params["owner"]["tab_id"], tab);
                }
            }
        }
    }
}

#[test]
fn lifecycle_rpcs_do_not_invent_identity_or_human_confirmation() {
    let _lock = crate::test_env_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let _env = CallerEnv::set(None, None, None);
    for (_, action) in lifecycle_actions() {
        let mut recorder = Recorder::default();
        run_plan(&mut recorder, true, action).unwrap();
        let params = &recorder.calls[0].1;
        for field in [
            "caller_tab_id",
            "session_id",
            "session_binding_nonce",
            "human_confirmation",
        ] {
            assert!(params.get(field).is_none());
        }
    }
}

#[test]
fn review_bounce_forwards_trusted_binding_and_keeps_from_as_attribution() {
    let _lock = crate::test_env_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    for (session, nonce) in [
        (None, Some("owner-nonce")),
        (Some("owner-session"), Some("owner-nonce")),
        (Some("owner-session"), None),
    ] {
        let _env = CallerEnv::set(Some("owner-tab"), session, nonce);
        let mut recorder = Recorder::default();
        run_plan(
            &mut recorder,
            true,
            PlanAction::Review {
                action: PlanReviewAction::Bounce {
                    plan_id: Some("pl_review".into()),
                    task_id: "task_review".into(),
                    note: Some("review the terminal generation".into()),
                    note_from_pad: None,
                    changed_executable_surfaces: true,
                    distinct_oracle: false,
                },
            },
        )
        .unwrap();

        assert_eq!(recorder.calls.len(), 1);
        let (method, params) = &recorder.calls[0];
        assert_eq!(method, "plan.review.bounce");
        assert_eq!(params["caller_tab_id"], "owner-tab");
        assert_eq!(
            params.get("session_id"),
            session.map(|value| json!(value)).as_ref()
        );
        assert_eq!(
            params.get("session_binding_nonce"),
            nonce.map(|value| json!(value)).as_ref()
        );
        assert_eq!(params["from"]["tab_id"], "owner-tab");
        assert_eq!(
            params["from"]["session_id"],
            session.map(|value| json!(value)).unwrap_or(Value::Null)
        );
        assert!(params["from"].get("session_binding_nonce").is_none());
    }
}

#[test]
fn plan_reads_do_not_gain_binding_authority() {
    let _lock = crate::test_env_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let _env = CallerEnv::set(Some("owner"), Some("session"), Some("nonce"));
    let mut recorder = Recorder::default();
    run_plan(
        &mut recorder,
        true,
        PlanAction::Get {
            plan_id: Some("pl_recovered".into()),
            sections: false,
        },
    )
    .unwrap();
    assert_eq!(
        recorder.calls,
        vec![(
            "plan.get".into(),
            json!({ "plan_id": "pl_recovered", "caller_tab_id": "owner" })
        )]
    );
}

#[test]
fn plan_rebind_attempt_reads_epoch_routes_exact_identity_and_sanitizes_success() {
    let _lock = crate::test_env_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let _env = CallerEnv::set(
        Some("worker-tab"),
        Some("provider-session"),
        Some("current-nonce"),
    );
    let mut client = AttemptRebindRecorder {
        calls: Vec::new(),
        response: json!({
            "ok": true,
            "plan_id": "pl_exact",
            "task_id": "task_exact",
            "attempt_id": "attempt_exact",
            "status": "restored",
            "session_binding_nonce": "private-current-nonce",
        }),
        fail_rebind: false,
    };
    let result =
        super::super::run_plan(&mut client, true, targeted_rebind_action(Some("pl_exact")))
            .unwrap()
            .unwrap();

    assert_eq!(client.calls.len(), 2);
    assert_eq!(
        client.calls[0],
        ("task.show".into(), json!({ "task_id": "COD-1" }))
    );
    assert_eq!(client.calls[1].0, "plan.session.rebind");
    assert_eq!(
        client.calls[1].1,
        json!({
            "plan_id": "pl_exact",
            "task_id": "task_exact",
            "attempt_id": "attempt_exact",
            "owner_epoch": 17,
            "caller_tab_id": "worker-tab",
            "session_id": "provider-session",
            "session_binding_nonce": "current-nonce",
        })
    );
    assert_eq!(result["status"], "restored");
    assert!(result.get("session_binding_nonce").is_none());
}

#[test]
fn plan_rebind_attempt_rejects_legacy_response_without_rendering_nonce_or_retrying() {
    let _lock = crate::test_env_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let _env = CallerEnv::set(
        Some("worker-tab"),
        Some("provider-session"),
        Some("current-nonce"),
    );
    let mut client = AttemptRebindRecorder {
        calls: Vec::new(),
        response: json!({
            "ok": true,
            "tab_id": "worker-tab",
            "session_binding_nonce": "private-current-nonce",
            "resynced": false,
        }),
        fail_rebind: false,
    };
    let error = super::super::run_plan(&mut client, true, targeted_rebind_action(Some("pl_exact")))
        .unwrap_err();
    assert_eq!(client.calls.len(), 2);
    assert!(error.contains("compatible daemon"));
    assert!(!error.contains("private-current-nonce"));
}

#[test]
fn plan_rebind_attempt_rpc_refusal_uses_the_shared_plan_error_renderer() {
    let _lock = crate::test_env_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let _env = CallerEnv::set(
        Some("worker-tab"),
        Some("provider-session"),
        Some("current-nonce"),
    );
    let mut client = AttemptRebindRecorder {
        calls: Vec::new(),
        response: Value::Null,
        fail_rebind: true,
    };
    let error = super::super::run_plan(&mut client, true, targeted_rebind_action(Some("pl_exact")))
        .unwrap_err();
    assert_eq!(client.calls.len(), 2);
    assert!(error.contains("restoration refused"));
}

#[test]
fn plan_rebind_attempt_requires_explicit_plan_before_rpc() {
    let _lock = crate::test_env_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let mut client = AttemptRebindRecorder {
        calls: Vec::new(),
        response: Value::Null,
        fail_rebind: false,
    };
    let error =
        super::super::run_plan(&mut client, true, targeted_rebind_action(None)).unwrap_err();
    assert!(error.contains("explicit plan id"));
    assert!(client.calls.is_empty());
}

#[test]
fn plan_rebind_attempt_refuses_a_task_from_another_plan_before_targeted_rpc() {
    let _lock = crate::test_env_lock()
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let mut client = AttemptRebindRecorder {
        calls: Vec::new(),
        response: Value::Null,
        fail_rebind: false,
    };
    let error = super::super::run_plan(&mut client, true, targeted_rebind_action(Some("pl_other")))
        .unwrap_err();
    assert!(error.contains("does not belong to the requested plan"));
    assert_eq!(client.calls.len(), 1);
}
