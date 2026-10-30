use crate::{
    blueprint_step::*,
    js_runtime::{JsViewProvider, PluginVm},
    plugin_jobs::*,
    view_host::{ViewCallContext, ViewError, ViewProvider},
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

struct Native;
#[async_trait]
impl ViewProvider for Native {
    fn id(&self) -> &str {
        "fixture"
    }
    fn job_lifecycle(&self) -> Option<JobLifecycleDescriptor> {
        Some(descriptor())
    }
    fn reconcile_job(&self, call: &JobCall) -> Result<JobObservation, ViewError> {
        Ok(JobObservation {
            job: call.context.job.clone(),
            state: OperationState::Running,
        })
    }
    fn cancel_job(&self, call: &JobCall) -> Result<JobObservation, ViewError> {
        Ok(JobObservation {
            job: call.context.job.clone(),
            state: OperationState::Unknown,
        })
    }
    async fn call(&self, _: &str, _: Value, _: &ViewCallContext) -> Result<Value, ViewError> {
        Ok(Value::Null)
    }
}

fn descriptor() -> JobLifecycleDescriptor {
    JobLifecycleDescriptor {
        contract_version: 1,
        reconcile: true,
        cancel: true,
    }
}
fn call() -> JobCall {
    JobCall {
        context: JobContext {
            scope: PluginJobScope {
                plugin: "fixture".into(),
                workspace: "workspace".into(),
                team: None,
                owner: "owner".into(),
            },
            origin: PluginJobOrigin::StudioView {
                surface: "studio".into(),
                instance: "one".into(),
                tab: "surface-tab".into(),
                frame: "closed-document".into(),
            },
            operation: "operation".into(),
            job: "exact-existing-job".into(),
        },
        deadline: Instant::now() + Duration::from_secs(5),
        cancellation: Default::default(),
    }
}

#[test]
fn native_and_js_plugin_job_lifecycle_observe_the_same_existing_identity_and_never_submit() {
    let source = r#"module.exports.default={jobReconcile(c){if(c.scope.owner!=="owner"||c.origin.frame!=="closed-document"||c.job!=="exact-existing-job"||c.operation!=="operation")throw Error("scope");return {job:c.job,state:"running"};},jobCancel(c){return {job:c.job,state:"unknown"};},submit(){throw Error("never called");}}"#;
    let vm = Arc::new(PluginVm::from_source("fixture", source.into()).unwrap());
    let undeclared = JsViewProvider::new(vm.clone(), "fixture");
    assert!(undeclared.reconcile_job(&call()).is_err());
    let provider = JsViewProvider::new(vm, "fixture").with_job_lifecycle(Some(descriptor()));
    assert_eq!(provider.job_lifecycle(), Native.job_lifecycle());
    assert_eq!(
        provider.reconcile_job(&call()).unwrap(),
        Native.reconcile_job(&call()).unwrap()
    );
    assert_eq!(
        provider.cancel_job(&call()).unwrap(),
        Native.cancel_job(&call()).unwrap()
    );
    let mut foreign = call();
    foreign.context.scope.plugin = "foreign".into();
    assert!(provider.reconcile_job(&foreign).is_err());
    let cancelled = call();
    cancelled.cancellation.cancel();
    assert!(provider.reconcile_job(&cancelled).is_err());
    let bad = Arc::new(PluginVm::from_source("fixture", r#"module.exports.default={jobReconcile(){return {job:"replacement",state:"succeeded"};}}"#.into()).unwrap());
    assert!(JsViewProvider::new(bad, "fixture")
        .with_job_lifecycle(Some(descriptor()))
        .reconcile_job(&call())
        .is_err());
    assert!(!serde_json::from_value::<JobObservation>(
        json!({"job":"exact-existing-job","state":"waiting_for_owner_ok"})
    )
    .unwrap()
    .valid_for("exact-existing-job"));
}
