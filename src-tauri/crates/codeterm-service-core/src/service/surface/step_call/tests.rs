use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};
use std::sync::Arc;

fn fixture() -> (ServiceFixture, StepInputContext) {
    let (mut mux, owner) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    mux.get_tab_mut(&owner).unwrap().agent_type = Some("fixture".into());
    let fixture = ServiceFixture::new(mux);
    fixture.service.ext::<SurfaceRegistryHandle>().register(
        "fixture",
        Arc::new(vec![serde_json::from_value(
            json!({"id":"page","title":"Page","renderer":"stream","runtime":"browser"}),
        )
        .unwrap()]),
    );
    let context = StepInputContext {
        plugin:"fixture".into(), submitting:true, deadline:std::time::Instant::now()+std::time::Duration::from_secs(10), cancellation:Default::default(),surfaces:Default::default(),
        request:serde_json::from_value(json!({"context":{"scope":{"workspace":workspace,"team":null,"run":"run"},"owner":owner,"node":"n","attempt":"a","operation":"op","nodeLabel":"n","inputLineage":[]},"nodeType":"fixture.read","revision":1,"params":{},"inputs":{}})).unwrap(),
    };
    (fixture, context)
}

#[test]
fn step_surface_never_targets_ui_surface_or_another_run() {
    let (fixture, context) = fixture();
    let service = &fixture.service;
    assert!(service
        .step_surface_call("fixture", &context, "status", "[]")
        .contains("no page open"));
    let foreign = service
        .open_surface(SurfaceOpenRequest {
            target: "fixture:page".into(),
            owner_tab_id: Some(context.request.context.owner.clone()),
            ..Default::default()
        })
        .unwrap();
    let failed: Value = serde_json::from_str(&service.step_surface_call(
        "fixture",
        &context,
        "open",
        r#"["--profile","../"]"#,
    ))
    .unwrap();
    assert!(failed["error"].is_string());
    assert!(context.surfaces.lock().is_empty());
    assert!(service.mux.read().get_tab(&foreign.tab_id).is_some());
    let before: Value = serde_json::from_str(
        &service.step_surface_call(
            "fixture",
            &context,
            "close",
            &json!({"args":[],"surface":{"tab":foreign.tab_id,"instance":foreign.instance_id}})
                .to_string(),
        ),
    )
    .unwrap();
    assert_eq!(before["error"], "surface was not opened by this step run");
    assert!(service.mux.read().get_tab(&foreign.tab_id).is_some());
    let other = StepInputContext {
        surfaces: Default::default(),
        ..context.clone()
    };
    context
        .surfaces
        .lock()
        .insert(foreign.tab_id.clone(), foreign.instance_id.clone());
    let denied: Value = serde_json::from_str(
        &service.step_surface_call(
            "fixture",
            &other,
            "text",
            &json!({"args":[],"surface":{"tab":foreign.tab_id,"instance":foreign.instance_id}})
                .to_string(),
        ),
    )
    .unwrap();
    assert_eq!(denied["error"], "surface was not opened by this step run");
}

#[test]
fn named_blueprint_profiles_require_owner_allowlist_before_open() {
    let (fixture, mut context) = fixture();
    let service = &fixture.service;
    let registry = service.ext::<SurfaceRegistryHandle>();
    registry.register("browser", registry.get("fixture").unwrap());
    context.plugin = "browser".into();
    context.request.node_type = "browser.read_page".into();
    context.request.params = json!({"url": "https://example.com", "profile": "Unlisted"});
    let answer: Value = serde_json::from_str(&service.step_surface_call(
        "browser",
        &context,
        "read_named_page",
        "[]",
    ))
    .unwrap();
    let refusal: Value = serde_json::from_str(answer["result"].as_str().unwrap()).unwrap();
    assert_eq!(refusal["outcome"], "profile_not_allowed");
    assert!(refusal["text"]
        .as_str()
        .unwrap()
        .contains("plugins.browser.blueprint_profiles"));
    assert_eq!(refusal["profile"], "");
    assert!(context.surfaces.lock().is_empty());

    service.ext::<crate::config::ConfigState>().lock().plugins.insert(
        "browser".into(),
        json!({"blueprint_profiles":["Unlisted"],"blueprint_origins":{"Unlisted":["https://example.com"]}}),
    );
    context.request.inputs.insert(
        "url".into(),
        vec![PortValue::Text {
            text: "https://other.example/path".into(),
        }],
    );
    let answer: Value = serde_json::from_str(&service.step_surface_call(
        "browser",
        &context,
        "read_named_page",
        "[]",
    ))
    .unwrap();
    let refusal: Value = serde_json::from_str(answer["result"].as_str().unwrap()).unwrap();
    assert_eq!(refusal["outcome"], "origin_not_allowed");
    assert!(context.surfaces.lock().is_empty());

    let bypass: Value = serde_json::from_str(&service.step_surface_call(
        "browser",
        &context,
        "open",
        r#"["--profile","Unlisted"]"#,
    ))
    .unwrap();
    assert!(bypass["error"].is_string());
    assert!(context.surfaces.lock().is_empty());
}
