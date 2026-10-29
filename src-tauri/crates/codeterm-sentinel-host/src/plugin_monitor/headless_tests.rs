use super::*;
use codeterm_plugin::{
    agent_commands::AgentCommandProvider,
    js_runtime::{JsAgentCommandHandler, JsViewProvider, PluginVm},
    plugin_jobs::{JobLifecycleDescriptor, PluginJobs},
    view_host::{ViewProvider, ViewRegistryHandle},
};
use codeterm_service_core::{
    multiplexer::state::MultiplexerState,
    service::{surface::SurfaceOpenRequest, test_fixture::ServiceFixture},
};
use serde_json::{json, Value};

fn fixture(
    root: &std::path::Path,
) -> (
    ServiceFixture,
    JsAgentCommandHandler,
    Arc<JsViewProvider>,
    String,
    String,
    Arc<dyn codeterm_plugin::PluginHostCtx>,
) {
    let (mut mux, owner) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&owner).unwrap();
    let foreign = mux.create_tab(&workspace).unwrap();
    for tab_id in [&owner, &foreign] {
        let tab = mux.get_tab_mut(tab_id).unwrap();
        tab.agent_type = Some("fixture-agent".into());
        tab.agent_session_id = Some(format!("session-{tab_id}"));
        tab.session_binding_nonce = Some(format!("nonce-{tab_id}"));
    }
    let fixture = ServiceFixture::new(mux);
    let service = &fixture.service;
    service.ext::<codeterm_plugin::surfaces::SurfaceRegistryHandle>().register("headless-fixture", Arc::new(vec![serde_json::from_value(json!({"id":"studio","title":"Studio","renderer":"view","agentVerbs":["admit","prepare","enter","status","track"],"stateInvalidation":true})).unwrap()]));
    assert!(service
        .register_host_extension(Arc::new(plugin::PluginStepStoreRoot(
            root.join("plugin-state")
        )))
        .is_ok());
    let clock = codeterm_cron::clock::MockClock::starting_at(1000);
    let handle = crate::scheduler::fixture_handle_without_app(crate::store::Store::new(
        root.join("sentinels.json"),
        clock,
    ));
    assert!(service.register_host_extension(Arc::new(handle)).is_ok());
    let weak = service.ext::<std::sync::Weak<ServiceContext>>().clone();
    assert!(service
        .register_host_extension(Arc::new(PluginJobs(Arc::new(
            crate::plugin_job_admission::JobAdmission(weak.clone())
        ))))
        .is_ok());
    let vm = Arc::new(PluginVm::from_source("headless-fixture", r#"module.exports.default={
      onAgentCommand(c){if(c.verb==='foreign-status')return host.surface.dispatch('status',{args:[c.args[0]],of:c.args[1]});return host.surface.dispatch(c.verb,c.args);},
      onSurfaceCall(context,verb,args){
        if(verb==='admit')return JSON.stringify({context:context,receipt:host.jobs.admit({owner:'foreign'})});
        if(verb==='prepare')return JSON.stringify(host.jobs.prepare(args[0],JSON.parse(args[1]),JSON.parse(args[2])));
        if(verb==='enter')return JSON.stringify(host.jobs.enter(args[0],JSON.parse(args[1])));
        if(verb==='track')return JSON.stringify(host.jobs.track(args[0],args[1]));
        return JSON.stringify(host.jobs.status(args[0]));
      },
      viewCall(method,args,context){return {context:context,status:host.jobs.status(args.operation)};}
    };"#.into()).unwrap());
    let host: Arc<dyn codeterm_plugin::PluginHostCtx> = weak.upgrade().unwrap();
    assert!(vm.set_service(Arc::downgrade(&host)));
    let provider = Arc::new(
        JsViewProvider::new(vm.clone(), "headless-fixture").with_job_lifecycle(Some(
            JobLifecycleDescriptor {
                contract_version: 1,
                reconcile: true,
                cancel: false,
            },
        )),
    );
    service
        .ext::<ViewRegistryHandle>()
        .register("headless-fixture", provider.clone());
    (
        fixture,
        JsAgentCommandHandler::new(vm, "headless-fixture", true),
        provider,
        owner,
        foreign,
        host,
    )
}
fn call(handler: &JsAgentCommandHandler, caller: &str, verb: &str, args: &[String]) -> Value {
    serde_json::from_str(
        &handler
            .on_agent_command(caller, verb, args, None)
            .unwrap()
            .result,
    )
    .unwrap()
}
fn quote(request: &Value) -> JobQuote {
    JobQuote {
        binding: job_request_binding(request).unwrap(),
        amount: 2,
        unit: "credits".into(),
        basis: "account tariff".into(),
        tariff_revision: "revision".into(),
        issued_at: 1000,
        valid_until_ms: 2000,
    }
}

#[tokio::test]
async fn headless_prepare_enter_and_later_view_share_host_owned_job_context() {
    let root = tempfile::tempdir().unwrap();
    let (fixture, handler, provider, owner, foreign, _host) = fixture(root.path());
    let service = &fixture.service;
    let before = service.mux.read().all_tabs().count();
    let expected_workspace = service.mux.read().find_workspace_for_tab(&owner).unwrap();
    let admitted = call(&handler, &owner, "admit", &["forged-owner".into()]);
    let operation = admitted["receipt"]["operation"].as_str().unwrap();
    let logical = service
        .plugin_agent_invocation("headless-fixture", "studio", &owner)
        .unwrap();
    assert_eq!(admitted["context"]["headless"], true);
    assert_eq!(admitted["context"]["jobContext"], logical.instance);
    assert_eq!(admitted["receipt"]["scope"]["owner"], owner);
    assert_eq!(
        admitted["receipt"]["scope"]["workspace"],
        expected_workspace
    );
    assert_eq!(admitted["receipt"]["origin"]["kind"], "agent_command");
    assert_eq!(service.mux.read().all_tabs().count(), before);
    assert!(service.list_surfaces().open.is_empty());
    let request = json!({"revision":1,"params":{"prompt":"opaque"},"inputs":[]});
    let prepared = call(
        &handler,
        &owner,
        "prepare",
        &[
            operation.into(),
            request.to_string(),
            json!(quote(&request)).to_string(),
        ],
    );
    assert_eq!(prepared["state"], "waiting_for_owner");
    assert_eq!(
        call(&handler, &foreign, "status", &[operation.into()])["error"],
        "plugin_job_request_refused"
    );
    assert_eq!(
        call(
            &handler,
            &foreign,
            "enter",
            &[operation.into(), request.to_string()]
        )["error"],
        "plugin_job_request_refused"
    );
    let ledger = plugin::ledger(service).unwrap();
    ledger
        .decide_job_spend("headless-fixture", operation, "Approve once", 1000)
        .unwrap();
    let entered = call(
        &handler,
        &owner,
        "enter",
        &[operation.into(), request.to_string()],
    );
    assert_eq!(entered["state"], "entered_unknown");
    assert_eq!(
        call(
            &handler,
            &owner,
            "enter",
            &[operation.into(), request.to_string()]
        )["error"],
        "plugin_job_request_refused"
    );
    let tracked = call(
        &handler,
        &owner,
        "track",
        &[operation.into(), "known-provider-job".into()],
    );
    assert_eq!(tracked["registered"], true);
    let opened = service
        .open_surface(SurfaceOpenRequest {
            target: "headless-fixture:studio".into(),
            owner_tab_id: Some(owner.clone()),
            caller_tab_id: Some(owner.clone()),
            ..Default::default()
        })
        .unwrap();
    let context = service
        .plugin_view_context(
            "headless-fixture",
            Some("studio"),
            Some(&opened.instance_id),
            Some(&opened.tab_id),
        )
        .unwrap();
    assert_eq!(
        context.job_context.as_deref(),
        Some(logical.instance.as_str())
    );
    let erased: Arc<dyn ViewProvider> = provider.clone();
    let registry = service.ext::<ViewRegistryHandle>();
    let frame = codeterm_plugin::media_delivery::create_frame(&erased, registry, context).unwrap();
    let context =
        codeterm_plugin::media_delivery::frame_context("headless-fixture", &frame.media_frame)
            .unwrap();
    let attached = provider
        .call("status", json!({"operation":operation}), &context)
        .await
        .unwrap();
    assert_eq!(attached["context"]["jobContext"], logical.instance);
    assert_eq!(attached["status"]["operation"], operation);
    assert_eq!(attached["status"]["job"], "known-provider-job");
    assert_eq!(attached["status"]["spend"], entered);
    assert_eq!(attached["status"]["origin"], admitted["receipt"]["origin"]);
    assert_eq!(
        call(
            &handler,
            &foreign,
            "foreign-status",
            &[operation.into(), owner.clone()]
        )["error"],
        "plugin_job_request_refused"
    );
}

#[test]
fn headless_paid_entry_refuses_changed_agent_team_session_or_plugin_registration() {
    let root = tempfile::tempdir().unwrap();
    let (fixture, handler, provider, owner, _, _host) = fixture(root.path());
    let service = &fixture.service;
    service.mux.write().get_tab_mut(&owner).unwrap().team_id = Some("trusted-team".into());
    let admitted = call(&handler, &owner, "admit", &[]);
    assert_eq!(admitted["receipt"]["scope"]["team"], "trusted-team");
    let operation = admitted["receipt"]["operation"].as_str().unwrap();
    let request = json!({"revision":1});
    call(
        &handler,
        &owner,
        "prepare",
        &[
            operation.into(),
            request.to_string(),
            json!(quote(&request)).to_string(),
        ],
    );
    let ledger = plugin::ledger(service).unwrap();
    ledger
        .decide_job_spend("headless-fixture", operation, "Approve once", 1000)
        .unwrap();
    service.mux.write().get_tab_mut(&owner).unwrap().team_id = Some("replacement-team".into());
    assert_eq!(
        call(
            &handler,
            &owner,
            "enter",
            &[operation.into(), request.to_string()]
        )["error"],
        "plugin_job_request_refused"
    );
    service.mux.write().get_tab_mut(&owner).unwrap().team_id = Some("trusted-team".into());
    service
        .mux
        .write()
        .get_tab_mut(&owner)
        .unwrap()
        .session_binding_nonce = Some("replacement-session".into());
    assert_eq!(
        call(
            &handler,
            &owner,
            "enter",
            &[operation.into(), request.to_string()]
        )["error"],
        "plugin_job_request_refused"
    );
    service
        .mux
        .write()
        .get_tab_mut(&owner)
        .unwrap()
        .session_binding_nonce = Some(format!("nonce-{owner}"));
    service
        .ext::<ViewRegistryHandle>()
        .register("headless-fixture", provider);
    assert_eq!(
        call(
            &handler,
            &owner,
            "enter",
            &[operation.into(), request.to_string()]
        )["error"],
        "plugin_job_request_refused"
    );
    assert_eq!(
        ledger
            .job_spend("headless-fixture", operation)
            .unwrap()
            .unwrap()
            .state,
        JobSpendState::ApprovedOnce
    );
}
