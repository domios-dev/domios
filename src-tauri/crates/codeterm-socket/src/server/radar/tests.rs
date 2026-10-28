use super::super::error_codes::{INVALID_PARAMS, NOT_FOUND};
use super::{dispatch, source_field, status_filter};
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::plan::{OrchestratorRef, Plan};
use codeterm_service_core::service::plan::radar::RadarStatus;
use codeterm_service_core::service::test_fixture::ServiceFixture;
use serde_json::json;
use std::sync::Arc;

fn fixture() -> ServiceFixture {
    let (mux, _) = MultiplexerState::new();
    ServiceFixture::new(mux)
}

#[test]
fn radar_rpc_adds_lists_edits_and_drops_a_workspace_item() {
    let fixture = fixture();
    let svc = Arc::new(fixture.service.clone());
    let project = fixture.temp.path().display().to_string();
    let added = dispatch(
        "radar.add",
        &Some(json!({
            "project": project,
            "title": "Noticed",
            "tags": ["ux"],
            "source_provider": "linear",
            "source_id": "ENG-7",
        })),
        &svc,
    )
    .unwrap();
    let id = added["item"]["id"].as_str().unwrap().to_string();

    let listed = dispatch(
        "radar.list",
        &Some(json!({ "project": project, "tag": "ux", "provider": "linear" })),
        &svc,
    )
    .unwrap();
    let edited = dispatch(
        "radar.update",
        &Some(json!({ "item_id": id, "title": "Renamed" })),
        &svc,
    )
    .unwrap();
    let dropped = dispatch("radar.drop", &Some(json!({ "item_id": id })), &svc).unwrap();
    let missing = dispatch("radar.show", &Some(json!({ "item_id": id })), &svc).unwrap_err();

    assert_eq!(added["created"], json!(true));
    assert_eq!(listed["items"].as_array().unwrap().len(), 1);
    assert_eq!(listed["items"][0]["id"], json!(id));
    assert_eq!(edited["item"]["title"], json!("Renamed"));
    assert_eq!(edited["item"]["tags"], json!(["ux"]));
    assert_eq!(dropped["dropped"]["id"], json!(id));
    assert_eq!(missing.code, NOT_FOUND);
}

#[test]
fn radar_pull_without_plan_authority_creates_no_task() {
    let fixture = fixture();
    let svc = Arc::new(fixture.service.clone());
    let project = fixture.temp.path().display().to_string();
    let mut plan = Plan::new(
        "Radar target".into(),
        project.clone(),
        OrchestratorRef::from_tab("owner"),
    );
    svc.plan_store.save(&mut plan, 1).unwrap();
    let added = dispatch(
        "radar.add",
        &Some(json!({ "project": project, "title": "Pull me" })),
        &svc,
    )
    .unwrap();
    let id = added["item"]["id"].as_str().unwrap().to_string();

    let refused = dispatch(
        "radar.pull",
        &Some(json!({ "item_id": id, "plan_id": plan.id, "caller_tab_id": "intruder" })),
        &svc,
    );

    assert!(refused.is_err());
    assert!(svc.plan_store.get(&plan.id).unwrap().tasks.is_empty());
    let shown = dispatch("radar.show", &Some(json!({ "item_id": id })), &svc).unwrap();
    assert_eq!(shown["item"]["status"], json!("open"));
}

#[test]
fn radar_list_status_defaults_to_open_and_rejects_unknown_values() {
    assert_eq!(status_filter(None).unwrap(), vec![RadarStatus::Open]);
    assert!(status_filter(Some("all")).unwrap().is_empty());
    assert_eq!(
        status_filter(Some("pulled")).unwrap(),
        vec![RadarStatus::Pulled]
    );
    assert_eq!(
        status_filter(Some("done")).unwrap_err().code,
        INVALID_PARAMS
    );
}

#[test]
fn radar_source_needs_provider_and_external_id_together() {
    let both = json!({ "source_provider": "github", "source_id": "acme/w#1" });
    let half = json!({ "source_provider": "github" });
    let url_only = json!({ "source_url": "https://example.com/1" });

    assert_eq!(
        source_field(Some(&both)).unwrap().unwrap().external_id,
        "acme/w#1"
    );
    assert!(source_field(Some(&json!({}))).unwrap().is_none());
    assert_eq!(source_field(Some(&half)).unwrap_err().code, INVALID_PARAMS);
    assert_eq!(
        source_field(Some(&url_only)).unwrap_err().code,
        INVALID_PARAMS
    );
}
