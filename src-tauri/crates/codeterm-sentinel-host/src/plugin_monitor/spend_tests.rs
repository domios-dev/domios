use super::*;
use codeterm_plugin::{
    plugin_jobs::PluginJobAdmission,
    view_host::{with_view_call_context, ViewError},
};
use serde_json::{json, Value};

#[test]
fn job_admission_exposes_host_scope_and_spend_refuses_absent_or_retired_document() {
    let root = tempfile::tempdir().unwrap();
    let (fixture, _, _, context, lease, _) = super::tests::fixture(root.path());
    let service = &fixture.service;
    let admission = crate::plugin_job_admission::JobAdmission(
        service.ext::<std::sync::Weak<ServiceContext>>().clone(),
    );
    let invoke = |request: Value| {
        with_view_call_context("monitor-fixture", &context, || {
            admission
                .invoke("monitor-fixture", &request.to_string())
                .map_err(ViewError::Call)
        })
    };
    let admitted = invoke(json!({"action":"admit","owner":"forged"}));
    assert!(admitted.is_err());
    let admitted = invoke(json!({"action":"admit"})).unwrap();
    assert_eq!(admitted["scope"]["owner"], json!(context.owner));
    assert_eq!(admitted["scope"]["workspace"], json!(context.workspace));
    assert_eq!(admitted["authorization"], "observation_only");
    let operation = admitted["operation"].as_str().unwrap();
    let request = json!({"revision":1,"params":{"prompt":"opaque"},"inputs":[]});
    let quote = JobQuote {
        binding: job_request_binding(&request).unwrap(),
        amount: 2,
        unit: "credits".into(),
        basis: "account tariff".into(),
        tariff_revision: "revision".into(),
        issued_at: 1000,
        valid_until_ms: 2000,
    };
    let prepared =
        invoke(json!({"action":"prepare","operation":operation,"request":request,"quote":quote}))
            .unwrap();
    assert_eq!(prepared["state"], "waiting_for_owner");
    assert!(invoke(json!({"action":"enter","operation":operation,"request":request})).is_err());
    let ledger = plugin::ledger(service).unwrap();
    let weak = service.ext::<std::sync::Weak<ServiceContext>>().clone();
    assert!(service
        .register_host_extension(Arc::new(
            codeterm_service_core::ports::daemon_question::DaemonQuestions::new(Arc::new(
                crate::plugin_owner::OwnerQuestions(weak),
            )),
        ))
        .is_ok());
    let question = service
        .publish_daemon_question(
            format!("plugin-job:monitor-fixture:{operation}"),
            "Paid operation".into(),
            "Exact quote".into(),
            vec![
                "Approve once".into(),
                "Set daily allowance".into(),
                "Decline".into(),
            ],
        )
        .unwrap();
    assert!(service.notification_requires_owner(&question));
    assert!(service
        .answer_notification(&question, "Approve once")
        .is_err());
    service
        .answer_notification_as_owner(&question, "Approve once")
        .unwrap();
    assert!(admission
        .invoke(
            "monitor-fixture",
            &json!({"action":"enter","operation":operation,"request":request}).to_string()
        )
        .is_err());
    codeterm_plugin::media_delivery::revoke_view_frame("monitor-fixture", &lease);
    assert!(invoke(json!({"action":"enter","operation":operation,"request":request})).is_err());
    assert_eq!(
        ledger
            .job_spend("monitor-fixture", operation)
            .unwrap()
            .unwrap()
            .state,
        JobSpendState::ApprovedOnce
    );
}

#[test]
fn budget_race_publishes_owner_question_and_expired_quote_projects_readmit_needed() {
    let root = tempfile::tempdir().unwrap();
    let (fixture, _, _, context, _, clock) = super::tests::fixture(root.path());
    let service = &fixture.service;
    let weak = service.ext::<std::sync::Weak<ServiceContext>>().clone();
    let admission = crate::plugin_job_admission::JobAdmission(weak.clone());
    assert!(service
        .register_host_extension(Arc::new(
            codeterm_service_core::ports::daemon_question::DaemonQuestions::new(Arc::new(
                crate::plugin_owner::OwnerQuestions(weak),
            )),
        ))
        .is_ok());
    let invoke = |request: Value| {
        with_view_call_context("monitor-fixture", &context, || {
            admission
                .invoke("monitor-fixture", &request.to_string())
                .map_err(ViewError::Call)
        })
    };
    let request = json!({"revision":1});
    let quote = JobQuote {
        binding: job_request_binding(&request).unwrap(),
        amount: 2,
        unit: "credits".into(),
        basis: "account tariff".into(),
        tariff_revision: "revision".into(),
        issued_at: 1000,
        valid_until_ms: 2000,
    };
    let first = invoke(json!({"action":"admit"})).unwrap();
    let initial = first["operation"].as_str().unwrap();
    invoke(json!({"action":"prepare","operation":initial,"request":request,"quote":quote}))
        .unwrap();
    let ledger = plugin::ledger(service).unwrap();
    ledger
        .decide_job_spend("monitor-fixture", initial, "Set daily allowance", 1000)
        .unwrap();
    let racing = invoke(json!({"action":"admit"})).unwrap();
    let operation = racing["operation"].as_str().unwrap();
    assert_eq!(
        invoke(json!({"action":"prepare","operation":operation,"request":request,"quote":quote}))
            .unwrap()["state"],
        "approved_budget"
    );
    let identity = ledger.job_origin("monitor-fixture", initial).unwrap();
    for _ in 0..10 {
        let next = ledger
            .admit_job_origin(identity.scope.clone(), identity.origin.clone(), 1000)
            .unwrap();
        ledger
            .prepare_job_spend(&next, &request, quote.clone(), 1000)
            .unwrap();
        ledger.enter_job_spend(&next, &request, 1000).unwrap();
    }
    let request_key = format!("plugin-job:monitor-fixture:{operation}");
    let has_question = || {
        let store = service.notif.lock();
        store
            .list(false)
            .into_iter()
            .find(|record| {
                store.answer_slot(&record.id).is_some_and(|slot| {
                    slot.daemon_request.as_deref() == Some(request_key.as_str())
                })
            })
            .map(|record| record.id.clone())
    };
    assert!(has_question().is_none());
    assert_eq!(
        invoke(json!({"action":"enter","operation":operation,"request":request})).unwrap()["state"],
        "waiting_for_owner"
    );
    let question = has_question().unwrap();
    clock.set_ms(2000);
    assert_eq!(
        invoke(json!({"action":"status","operation":operation})).unwrap()["spend"]["state"],
        "expired_readmit_needed"
    );
    assert_eq!(
        service
            .answer_notification_as_owner(&question, "Approve once")
            .unwrap_err(),
        "quote expired; re-admit needed"
    );
}
