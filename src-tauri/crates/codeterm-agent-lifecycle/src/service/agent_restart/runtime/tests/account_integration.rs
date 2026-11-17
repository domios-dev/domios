use super::*;
use crate::service::provider_accounts;
use std::time::Duration;

const ORIGINAL: &str = "opaque-original-goal";
const ACCOUNT_SOURCE: &str = r#"
module.exports = {
  runtimePlan(ctx, op) {
    return {action:{kind:'request',call:{method:op.kind,params:op}},state:null};
  },
  runtimeResponse(ctx, op, state, reply) {
    let result = {kind:'done'}, events = [];
    if (op.kind === 'initialize') {
      result = {kind:'initialized',binaryVersion:'fixture-runtime'};
      events = [{kind:'auth',state:'signedIn'}];
    }
    if (op.kind === 'resumeThread' || op.kind === 'openThread')
      result = {kind:'threadLoaded',threadId:op.threadId || ctx.bindingNonce,settings:op.settings};
    if (op.kind === 'readHistory')
      result = {kind:'historyPage',threadId:op.threadId,items:[],openItems:[]};
    if (op.kind === 'submitTurn') {
      const limited = String(reply.value.accountHome || '').replace(/\\/g,'/').endsWith('/account-a');
      const turn = limited ? 'limited-turn' : 'continued-turn';
      result = {kind:'turnAccepted',threadId:op.threadId,sendId:op.sendId,turnId:turn};
      if (limited) {
        const error = {kind:'chatItem',threadId:op.threadId,item:{id:'account-limit',kind:'system',
          content:'opaque-limit',systemSubtype:'api_error',toolFlowId:turn,
          toolInput:{retryable:false,retryableKind:null,retryAtMs:null,accountLimit:{}}}};
        const ended = {kind:'turnState',threadId:op.threadId,turnId:turn,state:'idle',detail:'failed'};
        events = [{kind:'turnState',threadId:op.threadId,turnId:turn,state:'working'},error,error,ended,ended];
      } else {
        const stale = {kind:'chatItem',threadId:op.threadId,item:{id:'account-limit',kind:'system',
          content:'opaque-limit',systemSubtype:'api_error',toolFlowId:'limited-turn',
          toolInput:{retryable:false,retryableKind:null,retryAtMs:null,accountLimit:{}}}};
        events = [{kind:'turnState',threadId:op.threadId,turnId:turn,state:'working'},stale,
          {kind:'turnState',threadId:op.threadId,turnId:'limited-turn',state:'idle',detail:'failed'}];
      }
    }
    return {kind:'complete',result,events};
  },
  runtimeReduce() { return {state:null,events:[]}; }
};
"#;

fn launch_account(fixture: &RuntimeFixture, account: &str, nonce: &str) {
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&fixture.tab).unwrap();
        tab.restorable_launch.account_id = Some(account.into());
        tab.restorable_launch.account_namespace = Some(account.into());
        tab.restorable_launch.runtime_launch_options = None;
        tab.agent_session_id = Some("own-thread".into());
        tab.session_binding_nonce = Some(nonce.into());
        tab.reported_session_binding_nonce = Some(nonce.into());
        tab.agent_runtime_state = None;
        tab.backing = TabBacking::ChatBackend {
            plugin_id: PROVIDER.into(),
            session_id: "own-thread".into(),
        };
    }
    agent_runtime::transport::launch(
        &fixture.service,
        PROVIDER,
        &fixture.tab,
        json!({"systemPrompt":PROMPT,"model":"supplied-model"}),
    )
    .unwrap();
}

fn add_other_account_a(
    fixture: &mut RuntimeFixture,
) -> (String, Arc<parking_lot::Mutex<Vec<Value>>>) {
    let tab = {
        let mut mux = fixture.service.mux.write();
        let workspace = mux.find_workspace_for_tab(&fixture.tab).unwrap().to_owned();
        let cwd = mux.get_tab(&fixture.tab).unwrap().cwd.clone();
        let id = mux.create_tab(&workspace).unwrap();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.agent_type = Some(PROVIDER.into());
        tab.agent_session_id = Some("other-thread".into());
        tab.session_binding_nonce = Some("other-nonce".into());
        tab.reported_session_binding_nonce = Some("other-nonce".into());
        tab.restorable_launch.account_id = Some("account-a".into());
        tab.restorable_launch.account_namespace = Some("account-a".into());
        tab.backing = TabBacking::ChatBackend {
            plugin_id: PROVIDER.into(),
            session_id: "other-thread".into(),
        };
        tab.cwd = cwd;
        id
    };
    let requests = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let (changed, _events) = tokio::sync::watch::channel(0);
    let ports = register_duplex_process(&tab, requests.clone(), changed, fixture.exits.clone());
    fixture.extra_processes.push((tab.clone(), ports));
    agent_runtime::transport::launch(
        &fixture.service,
        PROVIDER,
        &tab,
        json!({"systemPrompt":PROMPT,"model":"supplied-model"}),
    )
    .unwrap();
    (tab, requests)
}

#[test]
fn authenticated_account_limit_runs_scheduler_restart_and_one_durable_continuation() {
    let usual = tempfile::tempdir().unwrap();
    let mut fixture = RuntimeFixture::prepared_with_manifest(
        ACCOUNT_SOURCE,
        true,
        json!({}),
        Some(json!({
            "homeEnvironment":"ACCOUNT_ORACLE_HOME","defaultHome":usual.path().to_str().unwrap(),
            "secretEnvironment":[],"isolatedPaths":["auth.json"],"sharedPaths":[],"continuation":"proven"
        })),
    );
    let root = fixture.temp.path().join("isolated-account-root");
    let config_path = fixture.temp.path().join("isolated-account-config.toml");
    provider_accounts::test_support::configure(&fixture.service, root.clone(), config_path.clone())
        .unwrap();
    fixture
        .service
        .ext::<crate::config::ConfigState>()
        .lock()
        .agents
        .entry(PROVIDER.into())
        .or_default()
        .accounts = Some(
        serde_json::from_value(json!({"profiles":{"account-a":{},"account-b":{}},
            "order":["default","account-a","account-b"],"automatic_switch":true}))
        .unwrap(),
    );

    launch_account(&fixture, "account-b", "warm-b-nonce");
    assert_eq!(
        *agent_runtime::engine::for_tab(&fixture.tab)
            .unwrap()
            .auth
            .lock(),
        codeterm_plugin::agent_runtime::RuntimeAuthState::SignedIn
    );
    agent_runtime::engine::stop_blocking(&fixture.service, &fixture.tab).unwrap();
    launch_account(&fixture, "account-a", "account-a-nonce");
    let (other, other_requests) = add_other_account_a(&mut fixture);
    let other_runtime = agent_runtime::engine::for_tab(&other).unwrap();
    let other_process = other_runtime.process.lock().clone().unwrap();
    let other_before = fixture.service.mux.read().get_tab(&other).unwrap().clone();
    let other_home =
        agent_runtime::process::test_ports::spawned_env(&other)["ACCOUNT_ORACLE_HOME"].clone();
    let other_count = other_requests.lock().len();
    let original_runtime = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
    let original_process = original_runtime.process.lock().clone().unwrap();
    let count = fixture.requests().len();
    let mut receipt = provider_accounts::test_support::work_receipt(&fixture.service);
    let mut requests_changed = fixture.request_events.clone();
    agent_runtime::transport::send(
        &fixture.service,
        &fixture.tab,
        &crate::service::messenger::AgentInput::Human(
            crate::service::messenger::HumanInput::composer(ORIGINAL),
        ),
        None,
    )
    .unwrap();
    crate::async_runtime::block_on(async {
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                requests_changed.borrow_and_update();
                if fixture.requests()[count..]
                    .iter()
                    .filter(|request| request["method"] == "submitTurn")
                    .count()
                    == 2
                {
                    break;
                }
                requests_changed.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        provider_accounts::test_support::settled(&mut receipt)
            .await
            .unwrap();
    });

    let requests = fixture.requests();
    let switched = &requests[count..];
    assert_eq!(
        switched
            .iter()
            .map(|request| request["method"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![
            "submitTurn",
            "initialize",
            "resumeThread",
            "readHistory",
            "submitTurn"
        ]
    );
    assert_eq!(switched[0]["params"]["input"]["text"], ORIGINAL);
    assert_eq!(switched[2]["params"]["threadId"], "own-thread");
    assert_eq!(switched[4]["params"]["threadId"], "own-thread");
    assert_ne!(switched[4]["params"]["input"]["text"], ORIGINAL);
    assert!(original_process.ended.borrow().is_some());
    let next_home = agent_runtime::process::test_ports::spawned_env(&fixture.tab)
        ["ACCOUNT_ORACLE_HOME"]
        .clone();
    assert_ne!(next_home, other_home);
    assert!(std::path::Path::new(&next_home).starts_with(&root));
    assert!(std::path::Path::new(&next_home).ends_with("account-b"));
    assert_eq!(
        provider_accounts::transition(&fixture.service, &fixture.tab)
            .unwrap()
            .status,
        "continued"
    );
    {
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&fixture.tab).unwrap();
        assert_eq!(
            tab.restorable_launch.account_id.as_deref(),
            Some("account-b")
        );
        assert_eq!(tab.agent_session_id.as_deref(), Some("own-thread"));
        assert_eq!(tab.restorable_launch.account_notices.len(), 1);
        assert_eq!(
            tab.turn_resume
                .as_ref()
                .unwrap()
                .settled
                .as_ref()
                .unwrap()
                .outcome,
            crate::multiplexer::types::TurnResumeOutcome::Sent
        );
        let unchanged = mux.get_tab(&other).unwrap();
        assert_eq!(
            unchanged.restorable_launch.account_id,
            other_before.restorable_launch.account_id
        );
        assert_eq!(
            unchanged.session_binding_nonce,
            other_before.session_binding_nonce
        );
        assert_eq!(unchanged.agent_session_id, other_before.agent_session_id);
    }
    let persisted = crate::session::store::StorageDeskAdapter::from_storage(
        fixture._storage.as_ref().unwrap().clone(),
    )
    .snapshot()
    .unwrap();
    let restored = MultiplexerState::from_session_snapshot(&persisted);
    let restored_tab = restored.get_tab(&fixture.tab).unwrap();
    assert_eq!(
        restored_tab.restorable_launch.account_id.as_deref(),
        Some("account-b")
    );
    assert_eq!(
        restored_tab
            .turn_resume
            .as_ref()
            .unwrap()
            .settled
            .as_ref()
            .unwrap()
            .outcome,
        crate::multiplexer::types::TurnResumeOutcome::Sent
    );
    assert!(fixture
        .service
        .ext::<crate::config::ConfigState>()
        .lock()
        .agents[PROVIDER]
        .accounts
        .as_ref()
        .unwrap()
        .profiles["account-b"]
        .limit
        .is_none());
    assert!(config_path.is_file());
    assert!(other_process.ended.borrow().is_none());
    assert!(Arc::ptr_eq(
        &other_runtime,
        &agent_runtime::engine::for_tab(&other).unwrap()
    ));
    assert_eq!(other_requests.lock().len(), other_count);
    assert_eq!(
        agent_runtime::process::test_ports::spawned_env(&other)["ACCOUNT_ORACLE_HOME"],
        other_home
    );
}
