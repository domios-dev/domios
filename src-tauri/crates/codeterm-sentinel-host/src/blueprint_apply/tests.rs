use super::retarget_only;
use serde_json::json;

#[test]
fn curator_retarget_preserves_commands_thresholds_and_steps() {
    let before = json!({"schema_version":2,"kind":"pipeline","name":"retarget","trigger":{"type":"schedule","every":{"lit":"20s"}},"steps":[{"id":"read","run":{"lit":"opaque-command"}},{"id":"send","uses":"filter","with":{"pattern":{"lit":"opaque-pattern"}}}],"result":{"to":"tab:old"}});
    let mut after = before.clone();
    after["result"]["to"] = json!("tab:new");
    assert!(retarget_only(&before, &after));
    after["steps"][0]["run"] = json!({"lit":"changed-command"});
    assert!(!retarget_only(&before, &after));
    after = before.clone();
    after["steps"][1]["with"]["pattern"] = json!({"lit":"changed-pattern"});
    assert!(!retarget_only(&before, &after));
}

#[test]
fn curator_native_retarget_changes_only_declared_target_slots() {
    let before = json!({"schema_version":2,"kind":"watch","name":"retarget","steps":[{"id":"source","uses":"observe","with":{"kind":{"lit":"tab"},"id":{"lit":"old"}}},{"id":"decide","uses":"ask","with":{"question":{"lit":"Ready?"}},"buckets":["done"]},{"id":"effect","uses":"send","with":{"target":{"lit":"old"}}}]});
    let mut after = before.clone();
    after["steps"][0]["with"]["id"] = json!({"lit":"new"});
    after["steps"][2]["with"]["target"] = json!({"lit":"new"});
    assert!(retarget_only(&before, &after));
    after["steps"][0]["id"] = json!("new-node");
    assert!(!retarget_only(&before, &after));
    let before = json!({"schema_version":2,"kind":"pipeline","name":"opaque literal","steps":[{"id":"node","uses":"filter","with":{"pattern":{"lit":{"target":"old"}}}}]});
    let mut after = before.clone();
    after["steps"][0]["with"]["pattern"]["lit"]["target"] = json!("new");
    assert!(!retarget_only(&before, &after));
}

#[test]
fn blueprint_v2_check_host_rejects_same_graph_on_apply_import_update_then_admits_fixed() {
    use crate::{BlueprintApplyExt, BlueprintPortableExt};
    use codeterm_blueprint_types::{
        blueprint_document::{self, checker},
        blueprint_recipe::Recipe,
    };
    use codeterm_service_core::{
        multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
    };
    let (mut mux, tab) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&tab).unwrap();
    let team = codeterm_types::team::Team::new(&workspace, 1);
    mux.upsert_team(team.clone());
    mux.get_tab_mut(&tab).unwrap().team_id = Some(team.id.clone());
    let fixture = ServiceFixture::new(mux);
    let store = crate::store::Store::new(
        fixture.temp.path().join("checker.json"),
        codeterm_service_core::cron::clock::MockClock::starting_at(1000),
    );
    let handle = crate::scheduler::fixture_handle_with_ui(store, fixture.service.ui.clone());
    crate::manage_fixture_ports(&fixture.service, &handle);
    let caller = crate::service_adapter::SentinelCaller {
        tab,
        team: Some(team.id),
        workspace,
        cwd: fixture.temp.path().to_string_lossy().into(),
        nonce: None,
    };
    let source = include_str!("../../../../../tests/fixtures/blueprint-v2-check.yaml");
    let parsed = blueprint_document::parse("host.yaml", source).unwrap();
    let invalid = serde_json::to_value(&parsed.document).unwrap();
    let offline = blueprint_document::check("host.yaml", source, &[]);
    assert!(!offline.ok);
    assert_eq!(
        offline
            .diagnostics
            .iter()
            .filter(|d| d.severity == "error")
            .count(),
        1
    );
    for result in [
        fixture
            .service
            .blueprint_apply(&caller, invalid.clone(), false)
            .map_err(|e| e.to_string()),
        fixture.service.blueprint_import(
            &caller,
            Recipe {
                schema_version: 2,
                definition: invalid.clone(),
            },
            &[],
            true,
        ),
        fixture
            .service
            .blueprint_apply_document(
                &caller,
                invalid.clone(),
                false,
                None,
                Some(("missing".into(), 1, None)),
            )
            .map_err(|e| e.to_string()),
    ] {
        let error = result.unwrap_err();
        use std::io::Write;
        writeln!(
            std::io::stdout().lock(),
            "host admission refused; rows={}\n{}",
            handle.store.lock().jobs.len(),
            error
        )
        .unwrap();
        assert!(
            error.contains("S2") && error.contains("duration("),
            "{error}"
        );
        assert!(handle.store.lock().jobs.is_empty());
    }
    let repair = checker::fix::fix(&parsed, source, &checker::CheckContext::default());
    let checked = blueprint_document::check("host.yaml", repair.text.as_ref().unwrap(), &[]);
    assert!(checked.ok, "{:?}", checked.diagnostics);
    let result = fixture
        .service
        .blueprint_apply_with_state(&caller, checked.definition.unwrap(), false, Some(true))
        .unwrap();
    assert_eq!(result["action"], "create");
    assert_eq!(handle.store.lock().jobs.len(), 1);
    assert_eq!(
        handle.store.lock().jobs[0].document.as_ref().unwrap()["trigger"]["every"]["lit"],
        "10s"
    );
    let reopened = crate::store::Store::new(
        fixture.temp.path().join("checker.json"),
        codeterm_service_core::cron::clock::MockClock::starting_at(1000),
    );
    assert_eq!(reopened.jobs.len(), 1);
    assert_eq!(
        reopened.jobs[0].document.as_ref().unwrap()["trigger"]["every"]["lit"],
        "10s"
    );
    use std::io::Write;
    writeln!(
        std::io::stdout().lock(),
        "host admitted: {}\nreopened rows={} document={}",
        result,
        reopened.jobs.len(),
        reopened.jobs[0].document.as_ref().unwrap()
    )
    .unwrap();
}

#[test]
fn blueprint_v2_check_host_refuses_copy_under_another_name() {
    use crate::BlueprintApplyExt;
    use codeterm_service_core::{
        multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
    };
    for library in [false, true] {
        let (mut mux, tab) = MultiplexerState::new();
        let workspace = mux.find_workspace_for_tab(&tab).unwrap();
        let team = codeterm_types::team::Team::new(&workspace, 1);
        mux.upsert_team(team.clone());
        mux.get_tab_mut(&tab).unwrap().team_id = Some(team.id.clone());
        let fixture = ServiceFixture::new(mux);
        let store = crate::store::Store::new(
            fixture.temp.path().join("duplicates.json"),
            codeterm_service_core::cron::clock::MockClock::starting_at(1000),
        );
        let handle = crate::scheduler::fixture_handle_with_ui(store, fixture.service.ui.clone());
        crate::manage_fixture_ports(&fixture.service, &handle);
        let caller = crate::service_adapter::SentinelCaller {
            tab,
            team: Some(team.id),
            workspace,
            cwd: fixture.temp.path().to_string_lossy().into(),
            nonce: None,
        };
        let original = json!({"schema_version":2,"kind":"pipeline","name":"first-copy","owner":"team","trigger":{"type":"schedule","every":{"lit":"20s"}},"steps":[{"id":"read","run":{"lit":"echo ready"}}],"result":{"to":"notify"}});
        if library {
            fixture
                .service
                .mem
                .save_blueprint(
                    &format!("blueprints:{}", caller.workspace),
                    serde_json::from_value(original.clone()).unwrap(),
                    &["Report readiness".into()],
                    codeterm_blueprint_types::blueprint_memory::Evidence {
                        kept: true,
                        successes: 1,
                        ..Default::default()
                    },
                )
                .unwrap();
        } else {
            fixture
                .service
                .blueprint_apply(&caller, original.clone(), false)
                .unwrap();
        }
        let mut copy = original;
        copy["name"] = json!("second-copy");
        for dry_run in [true, false] {
            let error = fixture
                .service
                .blueprint_apply(&caller, copy.clone(), dry_run)
                .unwrap_err()
                .to_string();
            assert!(error.contains("R13"), "{error}");
            assert_eq!(handle.store.lock().jobs.len(), if library { 0 } else { 1 });
        }
    }
}
