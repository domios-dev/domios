use super::*;
use codeterm_plugin::{
    agent_commands::AgentCommandProvider,
    js_runtime::{JsAgentCommandHandler, JsViewProvider, PluginVm},
    plugin_jobs::{JobLifecycleDescriptor, PluginJobs},
    view_host::ViewRegistryHandle,
};
use serde_json::{json, Value};

#[test]
fn plugin_monitor_old_real_vm_cannot_borrow_replacement_registration_before_or_after_ticket_issue()
{
    for phase in ["before", "after"] {
        let root = tempfile::tempdir().unwrap();
        let (fixture, _, _, context, _, _) = super::tests::fixture(root.path());
        let owner = context.owner.unwrap();
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&owner)
            .unwrap()
            .agent_type = Some("fixture-agent".into());
        fixture.service.ext::<codeterm_plugin::surfaces::SurfaceRegistryHandle>().register("monitor-fixture",Arc::new(vec![serde_json::from_value(json!({"id":"studio","title":"Studio","renderer":"view","agentVerbs":["submit"],"stateInvalidation":true})).unwrap()]));
        assert!(fixture
            .service
            .register_host_extension(Arc::new(PluginJobs(Arc::new(
                crate::plugin_job_admission::JobAdmission(
                    fixture
                        .service
                        .ext::<std::sync::Weak<codeterm_service_core::service::ServiceContext>>()
                        .clone()
                )
            ))))
            .is_ok());
        let source = r#"module.exports.default={phase:'',onAgentCommand(c){if(c.verb==='raw'){host.surfaceCall('submit','[]');return {result:JSON.stringify(host.jobs.admit())};}this.phase=c.verb;if(this.phase==='before')host.progress({label:'park'});return host.surface.dispatch('submit',[]);},onSurfaceCall(){if(this.phase==='after')host.progress({label:'park'});return JSON.stringify(host.jobs.admit({owner:'forged',registration:'forged'}));}};"#;
        let old = Arc::new(PluginVm::from_source("monitor-fixture", source.into()).unwrap());
        let fresh = Arc::new(PluginVm::from_source("monitor-fixture", source.into()).unwrap());
        let host: Arc<dyn codeterm_plugin::PluginHostCtx> = fixture
            .service
            .ext::<std::sync::Weak<codeterm_service_core::service::ServiceContext>>()
            .clone()
            .upgrade()
            .unwrap();
        assert!(old.set_service(Arc::downgrade(&host)));
        assert!(fresh.set_service(Arc::downgrade(&host)));
        let provider = |vm| {
            Arc::new(
                JsViewProvider::new(vm, "monitor-fixture").with_job_lifecycle(Some(
                    JobLifecycleDescriptor {
                        contract_version: 1,
                        reconcile: true,
                        cancel: false,
                    },
                )),
            )
        };
        let registry = fixture.service.ext::<ViewRegistryHandle>();
        registry.register("monitor-fixture", provider(old.clone()));
        let (entered, observed) = std::sync::mpsc::channel();
        let (release, resume) = std::sync::mpsc::channel();
        let resume = Arc::new(parking_lot::Mutex::new(resume));
        let listener = fixture.ui.listen("plugin-activity", move |payload| {
            if payload["label"] == "park" {
                entered.send(()).unwrap();
                resume.lock().recv_timeout(Duration::from_secs(2)).unwrap();
            }
        });
        let actor = owner.clone();
        let phase = phase.to_string();
        let worker = std::thread::spawn(move || {
            JsAgentCommandHandler::new(old, "monitor-fixture", true).on_agent_command(
                &actor,
                &phase,
                &[],
                None,
            )
        });
        let parked = observed.recv_timeout(Duration::from_secs(2));
        registry.unregister("monitor-fixture");
        registry.register("monitor-fixture", provider(fresh.clone()));
        release.send(()).unwrap();
        let old_result = worker.join().unwrap();
        fixture.ui.unlisten(listener);
        assert!(parked.is_ok());
        if let Ok(result) = old_result {
            assert!(serde_json::from_str::<Value>(&result.result)
                .unwrap()
                .get("operation")
                .is_none());
        }
        let handler = JsAgentCommandHandler::new(fresh, "monitor-fixture", true);
        let result = handler
            .on_agent_command(&owner, "fresh", &[], None)
            .unwrap();
        let receipt: Value = serde_json::from_str(&result.result).unwrap();
        let operation = receipt["operation"].as_str().unwrap();
        assert_eq!(
            plugin::ledger(&fixture.service)
                .unwrap()
                .job_origin("monitor-fixture", operation)
                .unwrap()
                .scope
                .owner,
            owner
        );
        let raw = handler.on_agent_command(&owner, "raw", &[], None).unwrap();
        assert!(serde_json::from_str::<Value>(&raw.result)
            .unwrap()
            .get("operation")
            .is_none());
    }
}
