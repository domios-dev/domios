use super::*;
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

fn lifecycle_actions() -> Vec<(&'static str, PlanAction)> {
    vec![
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
        ("plan.resume", PlanAction::Resume { plan_id: "pl_recovered".into() }),
        ("plan.unarchive", PlanAction::Unarchive { plan_id: "pl_recovered".into() }),
        ("plan.disown", PlanAction::Disown { plan_id: "pl_recovered".into() }),
        ("plan.claim", PlanAction::Claim { plan_id: "pl_recovered".into() }),
    ]
}

#[test]
fn lifecycle_rpcs_forward_the_callers_binding_with_optional_session() {
    let _lock = crate::test_env_lock().lock().unwrap();
    for (tab, nonce) in [("recovered-owner", "owner-nonce"), ("foreign-tab", "foreign-nonce")] {
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
                assert_eq!(params.get("session_id"), session.map(|value| json!(value)).as_ref());
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
    let _lock = crate::test_env_lock().lock().unwrap();
    let _env = CallerEnv::set(None, None, None);
    for (_, action) in lifecycle_actions() {
        let mut recorder = Recorder::default();
        run_plan(&mut recorder, true, action).unwrap();
        let params = &recorder.calls[0].1;
        for field in ["caller_tab_id", "session_id", "session_binding_nonce", "human_confirmation"] {
            assert!(params.get(field).is_none());
        }
    }
}

#[test]
fn plan_reads_keep_their_request_free_of_caller_authority() {
    let _lock = crate::test_env_lock().lock().unwrap();
    let _env = CallerEnv::set(Some("owner"), Some("session"), Some("nonce"));
    let mut recorder = Recorder::default();
    run_plan(
        &mut recorder,
        true,
        PlanAction::Get { plan_id: Some("pl_recovered".into()), sections: false },
    ).unwrap();
    assert_eq!(recorder.calls, vec![("plan.get".into(), json!({ "plan_id": "pl_recovered" }))]);
}
