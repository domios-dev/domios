use super::*;
use crate::multiplexer::types::{SurfaceBacking, TabBacking};
use crate::service::test_fixture::ServiceFixture;

fn fixture() -> (ServiceFixture, String, String, String) {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let owner = mux.create_tab(&workspace).unwrap();
    let actor = mux.create_tab(&workspace).unwrap();
    for (id, label) in [(&owner, "Owner"), (&actor, "Visitor")] {
        let tab = mux.get_tab_mut(id).unwrap();
        tab.title = label.into();
        tab.agent_type = Some("opaque-agent".into());
    }
    let tab_id = mux.create_tab(&workspace).unwrap();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.parent_tab_id = Some(owner.clone());
    tab.backing = TabBacking::Surface(SurfaceBacking {
        plugin_id: "studio".into(),
        surface_id: "board".into(),
        instance_id: "instance-one".into(),
        owner_label: true,
        resume_url: None,
        resume_profile: None,
    });
    let fixture = ServiceFixture::new(mux);
    fixture.service.ext::<SurfaceRegistryHandle>().register(
        "studio",
        std::sync::Arc::new(vec![serde_json::from_value(
            json!({"id":"board","title":"Board","renderer":"view",
            "agentVerbs":["submit","status","annotate"],"stateInvalidation":true}),
        )
        .unwrap()]),
    );
    (fixture, owner, actor, tab_id)
}

#[test]
fn declared_view_verbs_stamp_two_agent_context_and_only_safe_activity() {
    for visitor in [false, true] {
        let (fixture, owner, actor, tab) = fixture();
        let caller = if visitor { &actor } else { &owner };
        let args = if visitor {
            json!({"of":owner,"args":["opaque-private-input"]})
        } else {
            json!(["opaque-private-input"])
        };
        let out: Value = serde_json::from_str(&fixture.service.surface_agent_call_json(
            "studio",
            Some(caller),
            "submit",
            &args.to_string(),
        ))
        .unwrap();
        let dispatch = &out["view_dispatch"];
        assert_eq!(
            dispatch["context"],
            json!({"surface":"board","instance":"instance-one","tab":tab,
            "owner":owner,"actor":caller,"actorLabel":if visitor {"Visitor"} else {"Owner"}})
        );
        assert_eq!(dispatch["args"], json!(["opaque-private-input"]));
        let events = fixture.ui.0.lock();
        let activity = events
            .iter()
            .find(|(name, _)| name == "surface-activity")
            .unwrap();
        assert_eq!(activity.1["tab_id"], tab);
        assert_eq!(activity.1["verb"], "submit");
        assert_eq!(
            activity.1.get("actor").and_then(Value::as_str),
            visitor.then_some("Visitor")
        );
        assert!(!serde_json::to_string(&*events)
            .unwrap()
            .contains("opaque-private-input"));
    }
}

#[test]
fn undeclared_and_spoofed_view_calls_have_zero_effects() {
    let (fixture, owner, visitor, _) = fixture();
    for (plugin, caller, verb, args) in [
        ("studio", owner.as_str(), "undeclared", "[]".into()),
        ("other-plugin", owner.as_str(), "submit", "[]".into()),
        (
            "studio",
            owner.as_str(),
            "submit",
            json!({"args":[],"actor":visitor}).to_string(),
        ),
        (
            "studio",
            visitor.as_str(),
            "close",
            json!({"of":owner}).to_string(),
        ),
        ("studio", owner.as_str(), "submit", "x".repeat(32769)),
    ] {
        let out: Value = serde_json::from_str(&fixture.service.surface_agent_call_json(
            plugin,
            Some(caller),
            verb,
            &args,
        ))
        .unwrap();
        assert!(out.get("error").is_some(), "{out}");
    }
    assert!(fixture.ui.0.lock().is_empty());
}

#[test]
fn view_instance_invalidation_requires_grant_exact_plugin_frame_and_live_instance() {
    let (fixture, _, _, tab) = fixture();
    let identity = json!({"surface":"board","instance":"instance-one","tab":tab});
    for (plugin, request) in [
        ("other-plugin", identity.clone()),
        (
            "studio",
            json!({"surface":"other","instance":"instance-one","tab":tab}),
        ),
        (
            "studio",
            json!({"surface":"board","instance":"stale","tab":tab}),
        ),
        (
            "studio",
            json!({"surface":"board","instance":"instance-one","tab":"foreign"}),
        ),
        (
            "studio",
            json!({"surface":"board","instance":"instance-one","tab":tab,"payload":"private"}),
        ),
    ] {
        assert!(fixture
            .service
            .invalidate_surface_view(plugin, &request.to_string())
            .is_err());
    }
    assert!(fixture
        .service
        .invalidate_surface_view("studio", &"x".repeat(1025))
        .is_err());
    assert!(fixture.ui.0.lock().is_empty());
    fixture
        .service
        .invalidate_surface_view("studio", &identity.to_string())
        .unwrap();
    let notices: Vec<_> = fixture
        .ui
        .0
        .lock()
        .iter()
        .filter(|(name, _)| name == "surface-state-invalidated")
        .cloned()
        .collect();
    assert_eq!(notices.len(), 1);
    assert_eq!(
        notices[0].1,
        json!({"plugin":"studio","surface":"board","instance":"instance-one","tab":tab})
    );
    fixture.service.mux.write().remove_tab(&tab).unwrap();
    assert!(fixture
        .service
        .invalidate_surface_view("studio", &identity.to_string())
        .is_err());
}

#[test]
fn view_invalidation_refuses_disabled_declaration_and_reopened_old_instance() {
    let (fixture, _, _, tab) = fixture();
    let identity = json!({"surface":"board","instance":"instance-one","tab":tab});
    {
        let mut mux = fixture.service.mux.write();
        let TabBacking::Surface(backing) = &mut mux.get_tab_mut(&tab).unwrap().backing else {
            panic!()
        };
        backing.instance_id = "instance-two".into();
    }
    assert!(fixture
        .service
        .invalidate_surface_view("studio", &identity.to_string())
        .is_err());
    let current = json!({"surface":"board","instance":"instance-two","tab":tab});
    fixture
        .service
        .invalidate_surface_view("studio", &current.to_string())
        .unwrap();
    let registry = fixture.service.ext::<SurfaceRegistryHandle>();
    let mut disabled = registry.get("studio").unwrap().to_vec();
    disabled[0].state_invalidation = false;
    registry.register("studio", std::sync::Arc::new(disabled));
    assert!(fixture
        .service
        .invalidate_surface_view("studio", &current.to_string())
        .is_err());
    registry.unregister("studio");
    assert!(fixture
        .service
        .invalidate_surface_view("studio", &current.to_string())
        .is_err());
}

#[test]
fn view_of_authorization_never_crosses_workspaces_and_open_close_stay_owner_only() {
    let (fixture, owner, visitor, tab) = fixture();
    let stranger = {
        let mut mux = fixture.service.mux.write();
        let other = mux.create_workspace("Other".into(), None).id;
        let id = mux.create_tab(&other).unwrap();
        mux.get_tab_mut(&id).unwrap().agent_type = Some("opaque-agent".into());
        id
    };
    let foreign = json!({"of":owner,"args":[]}).to_string();
    let out: Value = serde_json::from_str(&fixture.service.surface_agent_call_json(
        "studio",
        Some(&stranger),
        "submit",
        &foreign,
    ))
    .unwrap();
    assert!(out["error"].as_str().unwrap().contains("another workspace"));
    let mux = fixture.service.mux.read();
    let declarations = fixture
        .service
        .ext::<SurfaceRegistryHandle>()
        .get("studio")
        .unwrap();
    use super::super::agent_call::{plan_agent_call, CallPlan};
    assert_eq!(
        plan_agent_call(&mux, "studio", &declarations[0], &owner, "close", None).unwrap(),
        CallPlan::Close {
            tab_id: tab.clone()
        }
    );
    assert_eq!(
        plan_agent_call(&mux, "studio", &declarations[0], &visitor, "open", None).unwrap(),
        CallPlan::Open
    );
    assert!(plan_agent_call(
        &mux,
        "studio",
        &declarations[0],
        &visitor,
        "close",
        Some(&owner)
    )
    .is_err());
    assert!(validate_instance(
        &mux,
        "other-plugin",
        &ViewIdentity {
            surface: "board".into(),
            instance: "instance-one".into(),
            tab
        }
    )
    .is_err());
}

#[test]
fn foreign_view_open_never_dispatches_or_emits_activity_even_when_declared() {
    for declared_open in [false, true] {
        let (fixture, owner, visitor, tab) = fixture();
        if declared_open {
            let registry = fixture.service.ext::<SurfaceRegistryHandle>();
            let mut declared = registry.get("studio").unwrap().to_vec();
            declared[0].agent_verbs.push("open".into());
            registry.register("studio", std::sync::Arc::new(declared));
        }
        let tab_count = fixture.service.mux.read().all_tabs().count();
        let out: Value = serde_json::from_str(&fixture.service.surface_agent_call_json(
            "studio",
            Some(&visitor),
            "open",
            &json!({"of":owner,"args":["opaque-input"]}).to_string(),
        ))
        .unwrap();
        assert!(out["error"]
            .as_str()
            .unwrap()
            .contains("open only opens your own"));
        assert!(
            out.get("view_dispatch").is_none(),
            "no callback plan may cross the VM port"
        );
        assert!(
            fixture.ui.0.lock().is_empty(),
            "no activity or lifecycle effects"
        );
        let mux = fixture.service.mux.read();
        assert_eq!(mux.all_tabs().count(), tab_count);
        assert_eq!(
            mux.get_tab(&tab).unwrap().parent_tab_id.as_deref(),
            Some(owner.as_str())
        );
    }
}
