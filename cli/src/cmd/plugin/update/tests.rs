use super::*;

#[test]
fn absent_daemon_is_the_only_local_update_path() {
    assert!(route::<()>(Ok(None), |_| panic!("must stay local")).is_none());
    let refused = route::<()>(Err("permission denied".into()), |_| panic!("must refuse"));
    assert_eq!(refused, Some(Err("permission denied".into())));
}

#[test]
fn uncertain_update_never_becomes_a_local_replay() {
    let result = route(Ok(Some(())), |_| {
        Err("connection lost after update write".into())
    });
    assert_eq!(
        result,
        Some(Err("connection lost after update write".into()))
    );
}

#[test]
fn live_daemon_receives_directory_install_with_absolute_source_and_override() {
    let action = PluginAction::Install {
        id: None,
        from_dir: Some("local plugin".into()),
        id_override: Some("renamed".into()),
        channel: None,
    };
    let result = route_action(&action, Ok(Some("running-daemon")), |client, body| {
        assert_eq!(client, "running-daemon");
        assert_eq!(body["kind"], "install");
        assert_eq!(body["idOverride"], "renamed");
        assert!(body["pluginId"].is_null());
        let path = std::path::Path::new(body["fromDir"].as_str().unwrap());
        assert!(path.is_absolute());
        assert!(path.ends_with("local plugin"));
        Ok(json!({"id":"renamed"}))
    });
    assert_eq!(result, Some(Ok(json!({"id":"renamed"}))));
    assert!(route_action::<()>(&action, Ok(None), |_, _| panic!(
        "offline install stays local"
    ))
    .is_none());
    assert_eq!(
        route_action(
            &action,
            Ok(Some(())),
            |_, _| Err("uncertain install".into())
        ),
        Some(Err("uncertain install".into()))
    );
}

#[test]
fn channel_installs_and_all_update_forms_use_the_daemon() {
    for (action, kind, id) in [
        (
            PluginAction::Install {
                id: Some("fixture".into()),
                from_dir: None,
                id_override: None,
                channel: Some("fixture-channel".into()),
            },
            "install",
            Some("fixture"),
        ),
        (
            PluginAction::Update {
                id: Some("fixture".into()),
            },
            "update",
            Some("fixture"),
        ),
        (PluginAction::Update { id: None }, "update", None),
        (
            PluginAction::UpdateSystem { id: None },
            "update-system",
            None,
        ),
    ] {
        route_action(&action, Ok(Some(())), |_, body| {
            assert_eq!(body["kind"], kind);
            assert_eq!(body["pluginId"].as_str(), id);
            if kind == "install" {
                assert_eq!(body["channel"], "fixture-channel");
            }
            Ok(json!({"ok":true}))
        })
        .unwrap()
        .unwrap();
    }
}
