use super::*;
use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
use codeterm_plugin::provider::{AgentProvider, SessionContentQuery};
use std::{collections::HashMap, sync::Arc, time::Duration};

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn model_provider_fresh_pty_resume_keeps_tab_session_and_transcript() {
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let db = fixture.temp.path().join("opencode.db");
    let connection = rusqlite::Connection::open(&db).unwrap();
    connection.execute_batch(
        "CREATE TABLE session (id TEXT PRIMARY KEY, directory TEXT, title TEXT, time_created INTEGER, time_updated INTEGER);
         CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT);
         CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT, time_created INTEGER, time_updated INTEGER, data TEXT);
         INSERT INTO session VALUES ('ses_resume', '/work', 'fixture', 1000, 2000);
         INSERT INTO message VALUES ('m1', 'ses_resume', 1000, '{\"role\":\"user\"}');
         INSERT INTO part VALUES ('p1', 'm1', 'ses_resume', 1000, 1000, '{\"type\":\"text\",\"text\":\"opaque-turn\"}');",
    ).unwrap();
    drop(connection);
    let plugin =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../plugins/provider-opencode");
    let manifest: PluginManifest =
        serde_json::from_slice(&std::fs::read(plugin.join("plugin.json")).unwrap()).unwrap();
    let mut source = std::fs::read_to_string(plugin.join("plugin.js")).unwrap();
    source.push_str(&format!(
        "\nconst priorEnvGet = host.envGet; host.envGet = name => name === 'OPENCODE_DB' ? {} : priorEnvGet(name);",
        serde_json::to_string(db.to_str().unwrap()).unwrap(),
    ));
    let provider = JsProvider::new(
        manifest,
        Arc::new(PluginVm::from_source("opencode", source).unwrap()),
    );
    let query = SessionContentQuery {
        max_bytes: 65536,
        hint_terms: vec![],
    };
    let before = provider.read_session_content("ses_resume", &query).unwrap();
    assert_eq!(before.messages.len(), 1);
    assert_eq!(before.messages[0].text, "opaque-turn");
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(provider));
    let svc = &fixture.service;
    assert!(svc.model_provider_requires_fresh_pty("opencode"));
    let tab_id = {
        let mut mux = svc.mux.write();
        let workspace = mux.create_workspace("provider-relink".into(), None).id;
        let id = mux.create_tab(&workspace).unwrap();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.agent_type = Some("opencode".into());
        tab.agent_session_id = Some("ses_resume".into());
        let launch = crate::multiplexer::types::RestorableLaunchParameters {
            context_files: None,
            context_skills: None,
            worker_restart: None,
            captured: true,
            model: Some("endpoint/model".into()),
            requested_model: Some("endpoint/model".into()),
            args: Some(vec!["--agent".into(), "demo".into()]),
            ..Default::default()
        };
        tab.restorable_launch =
            serde_json::from_value(serde_json::to_value(launch).unwrap()).unwrap();
        id
    };
    let (old_id, old_session) = svc
        .pty
        .lock()
        .spawn(None, None, 80, 24, HashMap::new())
        .unwrap();
    let claim = svc.pty_ownership.acquire(&tab_id);
    let first = svc.link_pty_with_claim(&claim, &tab_id, old_id.clone(), true);
    assert!(first.linked);
    let snapshot = TabRestartSnapshot {
        tab_id: tab_id.clone(),
        pty_id: old_id.clone(),
        provider: "opencode".into(),
        target_provider: None,
        session_id: Some("ses_resume".into()),
        cwd: Some("/work".into()),
        auto_approve: false,
        role: None,
        role_profile: None,
        handoff_history: None,
    };
    let resumed = build_relaunch(svc, &snapshot, true).unwrap();
    assert!(resumed.command.contains("--session 'ses_resume'"));
    assert!(
        resumed.command.contains("'--agent' 'demo'") || resumed.command.contains("--agent 'demo'"),
        "{}",
        resumed.command
    );
    assert!(
        resumed.command.contains("'-m' 'endpoint/model'")
            || resumed.command.contains("-m 'endpoint/model'"),
        "{}",
        resumed.command
    );
    let (new_id, _) = svc
        .pty
        .lock()
        .spawn(None, None, 80, 24, HashMap::new())
        .unwrap();
    let replacement =
        svc.link_provider_launch_pty(&claim, &tab_id, new_id.clone(), Some(&old_id), true);
    assert!(replacement.linked && replacement.retained_candidate);
    assert_eq!(replacement.pty_id, new_id);
    {
        let mux = svc.mux.read();
        let tab = mux.get_tab(&tab_id).unwrap();
        assert_eq!(tab.pty_id.as_deref(), Some(new_id.as_str()));
        assert_eq!(tab.agent_session_id.as_deref(), Some("ses_resume"));
    }
    assert!(svc.pty.lock().get_session(&old_id).is_none());
    assert!(old_session
        .child_exit
        .wait_resolved(Duration::from_secs(10))
        .unwrap()
        .is_some());
    let after = svc
        .agent_registry
        .with_provider("opencode", |provider| {
            provider.read_session_content("ses_resume", &query).unwrap()
        })
        .unwrap();
    assert_eq!(after.messages.len(), before.messages.len());
    assert_eq!(after.messages[0].text, before.messages[0].text);
    assert_eq!(after.messages[0].role, before.messages[0].role);
    svc.kill_ptys(&[new_id]);
}
