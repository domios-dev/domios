use crate::js_runtime::{call_json, host_compute::tests::AnalyticsHost, PluginVm};
use crate::{blueprint_step::*, host_traits::PluginHostCtx, plugin_jobs::PluginJobAdmission};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
struct Admission {
    ledger: OperationLedger,
    now: AtomicU64,
    origin: JobOriginReceipt,
}
impl PluginJobAdmission for Admission {
    fn invoke(&self, plugin: &str, request: &str) -> Result<Value, String> {
        assert_eq!(plugin, self.origin.scope.plugin);
        let call: Value = serde_json::from_str(request).unwrap();
        if call["action"] == "binding" {
            return Ok(json!({"binding":job_request_binding(&call["request"])?}));
        }
        let operation = call["operation"].as_str().unwrap();
        assert_eq!(operation, self.origin.operation);
        let now = self.now.load(Ordering::SeqCst);
        let result = match call["action"].as_str().unwrap() {
            "prepare" => self.ledger.prepare_job_spend(
                &self.origin,
                &call["request"],
                serde_json::from_value(call["quote"].clone()).unwrap(),
                now,
            ),
            "enter" => self
                .ledger
                .enter_job_spend(&self.origin, &call["request"], now),
            _ => panic!("unexpected action"),
        }?;
        Ok(serde_json::to_value(result).unwrap())
    }
}
#[test]
fn native_and_js_job_spend_share_exact_request_scope_quote_and_one_shot_entry() {
    let root = tempfile::tempdir().unwrap();
    let ledger = OperationLedger::open(&root.path().join("ledger.sqlite")).unwrap();
    let origin = ledger
        .admit_job_origin(
            PluginJobScope {
                plugin: "spend-fixture".into(),
                workspace: "trusted-workspace".into(),
                team: None,
                owner: "trusted-owner".into(),
            },
            PluginJobOrigin::StudioAgent {
                surface: "studio".into(),
                instance: "one".into(),
                tab: "surface-tab".into(),
                actor: "trusted-actor".into(),
            },
            100,
        )
        .unwrap();
    let request = json!({"revision":1,"params":{"prompt":"opaque"},"inputs":[]});
    let quote = JobQuote {
        binding: job_request_binding(&request).unwrap(),
        amount: 2,
        unit: "credits".into(),
        basis: "account tariff".into(),
        tariff_revision: "revision".into(),
        issued_at: 100,
        valid_until_ms: 1000,
    };
    let native = ledger
        .prepare_job_spend(&origin, &request, quote.clone(), 100)
        .unwrap();
    let admission = Arc::new(Admission {
        ledger,
        origin: origin.clone(),
        now: AtomicU64::new(100),
    });
    let host: Arc<dyn PluginHostCtx> = Arc::new(AnalyticsHost {
        jobs: Some(admission.clone()),
        ..Default::default()
    });
    let vm = PluginVm::from_source("spend-fixture", "module.exports.default={binding(r){return host.jobs.requestBinding(r);},prepare(o,r,q){return host.jobs.prepare(o,r,q);},enter(o,r){return host.jobs.enter(o,r);}}".into()).unwrap();
    assert!(vm.set_service(Arc::downgrade(&host)));
    let binding: Value = call_json(&vm, "binding", &[request.clone()]).unwrap();
    assert_eq!(
        binding,
        json!({"binding":job_request_binding(&request).unwrap()})
    );
    let js: Value = call_json(
        &vm,
        "prepare",
        &[json!(origin.operation), request.clone(), json!(quote)],
    )
    .unwrap();
    assert_eq!(js, json!(native));
    admission.now.store(1000, Ordering::SeqCst);
    let expired: Value =
        call_json(&vm, "enter", &[json!(origin.operation), request.clone()]).unwrap();
    assert_eq!(expired, json!(native.clone().at(1000)));
    let expired_prepare: Value = call_json(
        &vm,
        "prepare",
        &[json!(origin.operation), request.clone(), json!(quote)],
    )
    .unwrap();
    assert_eq!(expired_prepare["state"], "expired_readmit_needed");
    admission.now.store(100, Ordering::SeqCst);
    let denied: Value =
        call_json(&vm, "enter", &[json!(origin.operation), request.clone()]).unwrap();
    assert_eq!(denied["error"], "plugin_job_request_refused");
    admission
        .ledger
        .decide_job_spend("spend-fixture", &origin.operation, "Approve once", 100)
        .unwrap();
    let entered: Value =
        call_json(&vm, "enter", &[json!(origin.operation), request.clone()]).unwrap();
    assert_eq!(
        entered,
        json!(admission
            .ledger
            .job_spend("spend-fixture", &origin.operation)
            .unwrap()
            .unwrap())
    );
    assert_eq!(entered["state"], "entered_unknown");
    assert_eq!(entered["scope"]["owner"], "trusted-owner");
    assert_eq!(entered["origin"]["actor"], "trusted-actor");
    assert!(admission
        .ledger
        .enter_job_spend(&origin, &request, 100)
        .is_err());
    let replay: Value = call_json(&vm, "enter", &[json!(origin.operation), request]).unwrap();
    assert_eq!(replay["error"], "plugin_job_request_refused");
}
