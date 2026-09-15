use std::sync::Arc;

use codeterm_types::team::Team;
use serde_json::{json, Value};

use crate::agent::kpi::attribution::now_epoch_secs;
use crate::agent::kpi::KpiRecorder;
use crate::agent::kpi_ledger::{
    KpiEventInput, KpiEventKind, KpiLedger, TabCreatedKind, TokenCounts,
};
use crate::plan::{OrchestratorRef, Plan};
use crate::service::test_fixture::ServiceFixture;
use crate::service::ServiceContext;

const PROBE_PLUGIN: &str =
    r#"module.exports.default = { probe(query) { return host.analytics.query(query); } }"#;

struct Scene {
    fixture: ServiceFixture,
    ledger: Arc<KpiLedger>,
    alpha: String,
    beta: String,
    today: String,
}

fn token_row(
    timestamp: u64,
    workspace: Option<&str>,
    plan: Option<&str>,
    uncached: u64,
) -> KpiEventInput {
    KpiEventInput {
        plan_id: plan.map(str::to_string),
        task_id: None,
        tab_id: "tab-analytics".into(),
        role: None,
        provider: "prov-a".into(),
        model: Some("model-a".into()),
        timestamp,
        kind: KpiEventKind::TokenUsage,
        bytes: uncached,
        tokens: Some(TokenCounts {
            input: uncached + 100,
            uncached_input: uncached,
            output: 10,
        }),
        dedup_key: None,
        workspace_id: workspace.map(str::to_string),
        tab_kind: None,
    }
}

fn scene() -> Scene {
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let mut fixture = ServiceFixture::new(mux);
    let ledger = Arc::new(KpiLedger::open_at(&fixture.temp.path().join("kpi.db")).unwrap());
    fixture.service.kpi = KpiRecorder::with_ledger(ledger.clone());
    let (alpha, beta) = {
        let mut mux = fixture.service.mux.write();
        let alpha = mux.create_workspace("Alpha".into(), None).id;
        let beta = mux.create_workspace("Beta".into(), None).id;
        mux.upsert_team(Team {
            id: "team_red".into(),
            name: Some("Red".into()),
            lifecycle: Default::default(),
            created_at: 0,
            workspace_id: alpha.clone(),
        });
        (alpha, beta)
    };
    let mut plan = Plan::new(
        "Teamed".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef {
            tab_id: None,
            session_id: None,
            title: None,
        },
    );
    plan.team_id = Some("team_red".into());
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    let now = now_epoch_secs();
    ledger
        .insert(&token_row(
            now,
            Some(alpha.as_str()),
            Some(plan.id.as_str()),
            400,
        ))
        .unwrap();
    ledger
        .insert(&token_row(now, Some(beta.as_str()), None, 250))
        .unwrap();
    ledger.insert(&token_row(now, None, None, 30)).unwrap();
    let today = chrono::Utc::now()
        .date_naive()
        .format("%Y-%m-%d")
        .to_string();
    Scene {
        fixture,
        ledger,
        alpha,
        beta,
        today,
    }
}

fn query(today: &str, group_by: &str, filters: Value) -> String {
    json!({
        "range": { "from": today, "to": today, "tz": "UTC" },
        "bucket": "day",
        "group_by": group_by,
        "filters": filters,
    })
    .to_string()
}

fn native_reply(service: &ServiceContext, query: &str) -> Value {
    serde_json::from_str(&service.analytics_snapshot_json(query)).unwrap()
}

fn js_reply(service: &ServiceContext, query: &str) -> Value {
    let host: Arc<dyn codeterm_plugin::PluginHostCtx> = Arc::new(service.clone());
    let vm = codeterm_plugin::js_runtime::PluginVm::from_source(
        "analytics-parity",
        PROBE_PLUGIN.to_string(),
    )
    .expect("probe vm");
    assert!(vm.set_service(Arc::downgrade(&host)));
    let parsed: Value = serde_json::from_str(query).unwrap();
    codeterm_plugin::js_runtime::call_json(&vm, "probe", &[parsed]).expect("probe call")
}

fn without_clock(mut reply: Value) -> Value {
    if let Some(object) = reply.as_object_mut() {
        object.remove("as_of");
    }
    reply
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn analytics_host_native_and_js_paths_agree_on_shared_fixtures() {
    let mut scene = scene();
    let queries = [
        query(&scene.today, "provider_model", json!({})),
        query(&scene.today, "workspace", json!({})),
        query(&scene.today, "team", json!({ "teams": ["team_red", null] })),
        query(&scene.today, "plan", json!({ "workspaces": [scene.alpha.clone(), null] })),
        query(&scene.today, "workspace", json!({ "workspaces": ["ws_on_a_peer"] })),
        r#"{"range":{"from":"2026-01-01","to":"2026-12-31","tz":"UTC"},"bucket":"week","group_by":"plan"}"#
            .to_string(),
        r#"{"range":{"from":"2026-09-01","to":"2026-09-01","tz":"Mars/Olympus"},"bucket":"day","group_by":"plan"}"#
            .to_string(),
    ];
    let assert_parity = |service: &ServiceContext| {
        for query in &queries {
            assert_eq!(
                without_clock(js_reply(service, query)),
                without_clock(native_reply(service, query)),
                "native and JS disagree on {query}"
            );
        }
    };

    assert_parity(&scene.fixture.service);
    let service = &scene.fixture.service;
    assert_eq!(
        native_reply(service, &queries[1])["recording"]["state"],
        "recording"
    );
    assert_eq!(native_reply(service, &queries[4])["error"], "foreign_scope");
    assert_eq!(native_reply(service, &queries[5])["error"], "invalid_query");
    assert_eq!(native_reply(service, &queries[6])["error"], "invalid_query");

    scene.fixture.service.kpi = KpiRecorder::disabled();
    assert_parity(&scene.fixture.service);
    let disabled = native_reply(&scene.fixture.service, &queries[0]);
    assert_eq!(disabled["recording"]["state"], "disabled");
    assert!(disabled["recording"]["enable_setting"].is_string());
    assert!(disabled["tokens"]["totals"].is_null());
    assert!(disabled["tabs_created"]["total"].is_null());

    let blocked = scene.fixture.temp.path().join("blocked-analytics-ledger");
    std::fs::write(&blocked, b"not a directory").unwrap();
    scene.fixture.service.kpi =
        KpiRecorder::from_config(&crate::config::schema::AppConfig::default(), &blocked);
    assert_parity(&scene.fixture.service);
    let unavailable = native_reply(&scene.fixture.service, &queries[0]);
    assert_eq!(unavailable["recording"]["state"], "unavailable");
    assert!(unavailable["recording"]["reason"].is_string());
    assert!(unavailable["recording"]["enable_setting"].is_null());
    assert!(unavailable["tokens"]["totals"].is_null());
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn analytics_host_reports_two_workspaces_teams_and_created_tabs_without_a_ledger_path() {
    let scene = scene();
    let service = &scene.fixture.service;
    service.create_tab(&scene.alpha).unwrap();
    service.create_tab(&scene.beta).unwrap();
    service.kpi.drain();

    let by_workspace = native_reply(service, &query(&scene.today, "workspace", json!({})));
    let groups = by_workspace["tokens"]["groups"].as_array().unwrap();
    let new_input = |id: Option<&str>| {
        groups
            .iter()
            .find(|group| group["key"]["id"].as_str() == id)
            .map(|group| group["totals"]["new_input"].clone())
    };
    assert_eq!(new_input(Some(scene.alpha.as_str())), Some(json!(400)));
    assert_eq!(new_input(Some(scene.beta.as_str())), Some(json!(250)));
    assert_eq!(new_input(None), Some(json!(30)));
    let alpha = groups
        .iter()
        .find(|group| group["key"]["id"].as_str() == Some(scene.alpha.as_str()))
        .unwrap();
    assert_eq!(alpha["label"], "Alpha");
    assert_eq!(by_workspace["tabs_created"]["total"], 2);
    assert_eq!(
        by_workspace["tabs_created"]["by_kind"],
        json!({ "agent": 0, "terminal": 2 })
    );

    let by_team = native_reply(service, &query(&scene.today, "team", json!({})));
    let teams: Vec<(Value, Value)> = by_team["tokens"]["groups"]
        .as_array()
        .unwrap()
        .iter()
        .map(|group| (group["key"]["id"].clone(), group["label"].clone()))
        .collect();
    assert!(teams.contains(&(json!("team_red"), json!("Red"))));
    assert!(
        teams.contains(&(Value::Null, Value::Null)),
        "rows charged to no plan stay in the no-team bucket"
    );

    let reply = service.analytics_snapshot_json(&query(&scene.today, "plan", json!({})));
    assert!(
        !reply.contains("kpi.db"),
        "the reply never names the ledger file"
    );
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn analytics_host_records_workspace_on_new_events_and_counts_created_tabs() {
    let scene = scene();
    let service = &scene.fixture.service;
    let tab = service.create_tab(&scene.beta).unwrap();
    service.record_kpi_event(&tab, KpiEventKind::Send, 12);
    service.kpi.drain();

    let rows = scene.ledger.list_all().unwrap();
    let created: Vec<_> = rows
        .iter()
        .filter(|row| row.kind == KpiEventKind::TabCreated)
        .collect();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].tab_id, tab);
    assert_eq!(
        created[0].workspace_id.as_deref(),
        Some(scene.beta.as_str())
    );
    assert_eq!(created[0].tab_kind, Some(TabCreatedKind::Terminal));
    assert_eq!(created[0].plan_id, None);
    let send = rows
        .iter()
        .find(|row| row.kind == KpiEventKind::Send)
        .unwrap();
    assert_eq!(send.workspace_id.as_deref(), Some(scene.beta.as_str()));
}

#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn analytics_host_repeat_queries_reuse_the_cached_read_until_a_row_commits() {
    let scene = scene();
    let service = &scene.fixture.service;
    let before = scene.ledger.analytics_reads();
    native_reply(service, &query(&scene.today, "workspace", json!({})));
    native_reply(
        service,
        &query(
            &scene.today,
            "provider_model",
            json!({ "providers": ["prov-a"] }),
        ),
    );
    assert_eq!(
        scene.ledger.analytics_reads(),
        before + 1,
        "a grouping or filter change folds the cached read"
    );

    scene
        .ledger
        .insert(&token_row(
            now_epoch_secs(),
            Some(scene.alpha.as_str()),
            None,
            5,
        ))
        .unwrap();
    let after = native_reply(service, &query(&scene.today, "workspace", json!({})));
    assert_eq!(scene.ledger.analytics_reads(), before + 2);
    assert_eq!(after["tokens"]["totals"]["new_input"], 685);
}
