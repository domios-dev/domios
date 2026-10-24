use super::*;
use crate::multiplexer::{state::MultiplexerState, types::TabBacking};
use crate::service::{agent_runtime::engine, test_fixture::ServiceFixture};
use serde_json::{json, Value};
use std::sync::atomic::Ordering;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[test]
fn general_runtime_captures_thread_instructions_and_bootstrap_first_turn() {
    for version in ["0.163.0", "0.164.0"] {
        capture_launch(version, Some("fragment-b72\nλ $() `opaque`\nfragment-c38"));
        capture_launch(version, None);
    }
}

fn capture_launch(version: &str, bootstrap: Option<&str>) {
    const PROVIDER: &str = "opaque-runtime-provider";
    const INSTRUCTIONS: &str = "fragment-a19\nopaque-role-δ";
    let plugins =
        crate::test_support::fixture_manifest_dir().join("../../../plugins/provider-codex");
    let mut manifest: PluginManifest =
        serde_json::from_str(&std::fs::read_to_string(plugins.join("plugin.json")).unwrap())
            .unwrap();
    manifest.id = PROVIDER.into();
    let vm = PluginVm::from_source(
        PROVIDER,
        std::fs::read_to_string(plugins.join("plugin.js")).unwrap(),
    )
    .unwrap();
    let provider = JsProvider::new(manifest, Arc::new(vm));
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some(PROVIDER.into());
    tab.agent_session_id = Some("fixture-thread".into());
    tab.session_binding_nonce = Some(uuid::Uuid::new_v4().to_string());
    tab.cwd = Some(plugins.to_string_lossy().into_owned().into());
    tab.backing = TabBacking::ChatBackend {
        plugin_id: PROVIDER.into(),
        session_id: String::new(),
    };
    crate::service::general_agent::identity::apply_general_agent_kind(tab);
    let prepared = tab.clone();
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(provider));
    fixture.install_seeded_storage_desk();
    let adapter = fixture
        .service
        .agent_registry
        .provider_arc(PROVIDER)
        .unwrap();
    let mut live = engine::test_support::loaded_runtime(&prepared, Some(adapter));
    let peer = std::mem::replace(&mut live.peer, tokio::io::duplex(1).0);
    let session = engine::for_tab(&id).unwrap();
    session.state.lock().phase = crate::service::agent_runtime::model::RuntimePhase::Loading;
    session.state.lock().thread_id = None;
    session.context.lock().thread_id = None;
    session.thread_loaded.store(false, Ordering::Release);
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&id)
        .unwrap()
        .agent_session_id = None;
    let version = version.to_owned();
    let expected_bootstrap = bootstrap.map(str::to_owned);
    let capture = crate::async_runtime::spawn(async move {
        let mut peer = BufReader::new(peer);
        let mut frames = Vec::new();
        loop {
            let mut line = String::new();
            tokio::time::timeout(
                std::time::Duration::from_secs(10),
                peer.read_line(&mut line),
            )
            .await
            .unwrap()
            .unwrap();
            let frame: Value = serde_json::from_str(&line).unwrap();
            let method = frame["method"].as_str().unwrap();
            if method == "initialized" {
                continue;
            }
            let result = match method {
                "initialize" => json!({"userAgent":format!("opaque/{version}")}),
                "account/read" => json!({"account":{"type":"apiKey"}}),
                "thread/start" => json!({"thread":{"id":"fixture-thread"}}),
                "thread/turns/list" => json!({"data":[],"nextCursor":null}),
                "thread/goal/get" => json!({"goal":null}),
                "turn/start" => json!({"turn":{"id":"fixture-turn"}}),
                _ => panic!("Unexpected request: {method}"),
            };
            let finished = method == "turn/start"
                || method == "thread/goal/get" && expected_bootstrap.is_none();
            let response = json!({"id":frame["id"],"result":result});
            frames.push(frame);
            peer.get_mut()
                .write_all(format!("{response}\n").as_bytes())
                .await
                .unwrap();
            if finished {
                return (frames, peer);
            }
        }
    });
    prepare_bootstrap(&fixture.service, &id, bootstrap).unwrap();
    let metadata = ChatBackendLaunchMetadata {
        agent: AgentSpawnSpec {
            provider: PROVIDER.into(),
            role: Some("Manager".into()),
            role_profile: Some(crate::service::general_agent::GENERAL_AGENT_KIND.into()),
            ..Default::default()
        },
        params: crate::service::SpawnAgentParams {
            agent: Some(PROVIDER.into()),
            skip_permissions: Some(false),
            ..Default::default()
        },
        owner_linkage: None,
        runtime_fork: None,
    };
    fixture
        .service
        .launch_chatbackend_runtime_existing(
            &id,
            &metadata,
            runtime_config(INSTRUCTIONS.into(), None, None, false),
        )
        .unwrap();
    let (frames, _peer) = crate::async_runtime::block_on(capture).unwrap();
    let start = frames
        .iter()
        .find(|frame| frame["method"] == "thread/start")
        .unwrap();
    assert_eq!(start["params"]["developerInstructions"], INSTRUCTIONS);
    assert!(start["params"].get("baseInstructions").is_none());
    let turns: Vec<_> = frames
        .iter()
        .filter(|frame| frame["method"] == "turn/start")
        .collect();
    assert_eq!(turns.len(), usize::from(bootstrap.is_some()));
    if let Some(bootstrap) = bootstrap {
        assert_eq!(
            turns[0]["params"]["input"],
            json!([{"type":"text","text":bootstrap}])
        );
        assert_eq!(turns[0]["params"]["threadId"], "fixture-thread");
    }
    drop(live);
}
