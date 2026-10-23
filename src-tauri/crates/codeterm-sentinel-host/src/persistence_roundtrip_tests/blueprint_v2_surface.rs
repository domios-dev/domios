use super::{close, ServiceInstance};
use crate::{store::Store, BlueprintApplyExt, BlueprintPortableExt, SentinelExt};
use codeterm_blueprint_types::blueprint_document::{self, Document};
use serde_json::{json, Value};

fn native(source: &str, set: &[String]) -> Value {
    let check = blueprint_document::check("workflow.yaml", source, set);
    assert!(check.ok, "{:?}", check.diagnostics);
    check.definition.unwrap()
}
#[test]
fn blueprint_v2_surface_check_apply_export_update_and_reopen_refuses_v1_before_mutation() {
    let root = tempfile::tempdir().unwrap();
    let (mut mux, tab) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&tab).unwrap();
    let team = codeterm_types::team::Team::new(&workspace, 1);
    mux.upsert_team(team.clone());
    mux.get_tab_mut(&tab).unwrap().team_id = Some(team.id.clone());
    mux.get_tab_mut(&tab).unwrap().agent_type = Some("opaque-provider".into());
    mux.get_tab_mut(&tab).unwrap().role = Some(codeterm_types::agent::AgentRole::Manager);
    mux.get_tab_mut(&tab).unwrap().role_profile = Some("orchestrator".into());
    mux.get_tab_mut(&tab).unwrap().cwd = Some(root.path().to_string_lossy().into_owned());
    let instance = ServiceInstance::open_with_mux(root.path(), mux);
    let svc = &instance.fixture.service;
    let handle = crate::scheduler::fixture_handle_with_ui(
        Store::with_storage(
            &instance.storage,
            codeterm_service_core::cron::clock::system_clock(),
        ),
        svc.ui.clone(),
    );
    crate::manage_fixture_ports(svc, &handle);
    let caller = svc.sentinel_caller(&tab, None).unwrap();
    let echo = native(
        include_str!("../../../../../resources/blueprints/echo.yaml"),
        &["message=opaque-message".into()],
    );
    let watch = native(
        include_str!("../../../../../resources/blueprints/all-finished.yaml"),
        &[
            "name=workflow-watch".into(),
            "one_verdict_probe=echo alldone".into(),
            "all_terminal_pattern=alldone".into(),
        ],
    );
    let mut saved = Vec::new();
    for definition in [echo, watch] {
        let preview = svc
            .blueprint_apply(&caller, definition.clone(), true)
            .unwrap();
        assert_eq!(preview["action"], "create");
        let before = handle.store.lock().jobs.len();
        let result = svc
            .blueprint_apply(&caller, definition.clone(), false)
            .unwrap();
        assert_eq!(handle.store.lock().jobs.len(), before + 1);
        let id = result["id"].as_str().unwrap().to_owned();
        assert_eq!(
            svc.sentinel_show(&caller, &id).unwrap().document,
            Some(definition.clone())
        );
        assert_eq!(
            svc.blueprint_apply(&caller, definition.clone(), false)
                .unwrap()["action"],
            "unchanged"
        );
        let exported = svc.blueprint_export(&caller, &id).unwrap();
        assert_eq!(exported.schema_version, 2);
        assert_eq!(exported.definition["schema_version"], 2);
        assert!(Document::from_recipe(&exported)
            .unwrap()
            .yaml()
            .unwrap()
            .contains("schema_version: 2"));
        let assignments = if definition["kind"] == "pipeline" {
            vec!["source=opaque-message".into()]
        } else {
            vec!["command=echo alldone".into()]
        };
        assert_eq!(
            svc.blueprint_import(&caller, exported, &assignments, false)
                .unwrap()["id"],
            id
        );
        saved.push((id, definition));
    }
    let before = serde_json::to_value(&handle.store.lock().jobs).unwrap();
    for invalid in [
        json!({"schema_version":1,"name":"old","watch":{}}),
        json!({"create":{"name":"old"}}),
        json!({"spec":{"name":"old"}}),
    ] {
        assert!(svc
            .blueprint_apply(&caller, invalid.clone(), false)
            .is_err());
        assert!(svc
            .blueprint_import(
                &caller,
                codeterm_blueprint_types::blueprint_recipe::Recipe {
                    schema_version: 2,
                    definition: invalid.clone()
                },
                &[],
                true
            )
            .is_err());
        assert!(svc
            .blueprint_apply_document(
                &caller,
                invalid,
                false,
                None,
                Some((saved[0].0.clone(), 1, None))
            )
            .is_err());
    }
    let mut unsupported = saved[0].1.clone();
    unsupported["steps"] = json!([{"id":"code","code":{"lang":"python","src":"raise RuntimeError('must never execute')","caps":[],"limits":{}}}]);
    assert!(svc
        .blueprint_apply(&caller, unsupported, false)
        .unwrap_err()
        .to_string()
        .contains("code execution"));
    let mut unsupported = saved[0].1.clone();
    unsupported["steps"] = json!([{"id":"call","call":{"ref":"missing","with":{}}}]);
    assert!(svc
        .blueprint_apply(&caller, unsupported, false)
        .unwrap_err()
        .to_string()
        .contains("call execution"));
    assert_eq!(
        before,
        serde_json::to_value(&handle.store.lock().jobs).unwrap()
    );
    let (id, definition) = &mut saved[0];
    definition["note"] = json!("Updated native explanation");
    assert!(svc
        .blueprint_apply_document(
            &caller,
            definition.clone(),
            false,
            None,
            Some((id.clone(), 99, None))
        )
        .is_err());
    assert_eq!(
        svc.blueprint_apply_document(
            &caller,
            definition.clone(),
            false,
            None,
            Some((id.clone(), 1, None))
        )
        .unwrap()["definition_version"],
        2
    );
    drop(handle);
    let reopened = instance.restart(root.path());
    let store = Store::with_storage(
        &reopened.storage,
        codeterm_service_core::cron::clock::system_clock(),
    );
    store.ensure_available().unwrap();
    assert_eq!(store.jobs.len(), 2);
    for (id, definition) in saved {
        assert_eq!(store.get(&id).unwrap().document, Some(definition));
    }
    drop(store);
    close(reopened);
}

#[test]
fn blueprint_v2_surface_memory_cleanup_preserves_v2_and_unrelated_records_and_shared_blobs() {
    use codeterm_service_core::mem::{MemKind, MemManager, MemScope, NewMemItem};
    let root = tempfile::tempdir().unwrap();
    let mem = MemManager::open_offline(root.path()).unwrap();
    let old = format!(
        "schema_version: 1\nname: old\nnote: {}\nwatch: {{}}\n",
        "old ".repeat(800)
    );
    let v2 = include_str!("../../../../../resources/blueprints/echo.yaml").to_owned();
    let mut ids = Vec::new();
    for (source, body, kind) in [
        ("blueprint-library", old.clone(), MemKind::Skill),
        ("blueprint-library", v2.clone(), MemKind::Skill),
        ("unrelated", old, MemKind::Note),
    ] {
        ids.push(
            mem.insert_raw(&NewMemItem {
                kind,
                scope: MemScope::Global,
                title: "cleanup fixture".into(),
                body,
                source: Some(source.into()),
                summary: None,
                area: None,
                tags: Vec::new(),
                importance: 3.0,
                source_path: None,
                pinned: false,
            })
            .unwrap(),
        );
    }
    let conn = rusqlite::Connection::open(root.path().join("knowledge.db")).unwrap();
    let metadata=json!({"content_fingerprint":null,"blueprint":{"intents":["fixture intent"],"successes":1,"failures":0,"uses":0,"last_used":0,"explicitly_kept":true},"curator":null,"evidence":{}}).to_string();
    for id in &ids[..2] {
        conn.execute(
            "INSERT INTO solution_metadata(item_id,metadata_json) VALUES (?1,?2)",
            rusqlite::params![id, metadata],
        )
        .unwrap();
        conn.execute("INSERT INTO blueprint_index(item_id,document_hash,canon,gists,signature_json) VALUES (?1,'hash','canon','gists','{}')",[id]).unwrap();
    }
    let old_blob: String = conn
        .query_row(
            "SELECT body_blob_path FROM mem_items WHERE id=?1",
            [&ids[0]],
            |r| r.get(0),
        )
        .unwrap();
    drop(conn);
    drop(mem);
    for _ in 0..2 {
        let reopened = MemManager::open_offline(root.path()).unwrap();
        assert!(reopened.get(&ids[0]).unwrap().is_none());
        assert_eq!(reopened.get(&ids[1]).unwrap().unwrap().body, v2);
        assert!(reopened.get(&ids[2]).unwrap().is_some());
        assert!(root.path().join("blobs").join(&old_blob).exists());
        let conn = rusqlite::Connection::open(root.path().join("knowledge.db")).unwrap();
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM blueprint_index WHERE item_id=?1",
                [&ids[0]],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM blueprint_index WHERE item_id=?1",
                [&ids[1]],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM mem_item_history WHERE item_id=?1",
                [&ids[0]],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
            0
        );
        drop(conn);
        drop(reopened);
    }
}
