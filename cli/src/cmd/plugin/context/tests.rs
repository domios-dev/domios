use super::*;
use crate::cmd::plugin::convert_action;
use serde_json::json;

fn install(from_dir: Option<String>) -> PluginAction {
    PluginAction::Install {
        id: Some("needy".into()),
        from_dir,
        id_override: None,
        channel: Some("fixture".into()),
    }
}

fn host_context(root: PathBuf, action: &PluginAction) -> PluginCtx {
    let ctx = resolve_with(root, action, || {
        Ok(json!({"version": "1.19.2", "cli": {"version": "99.0.0"}}))
    })
    .unwrap();
    assert_eq!(ctx.version.as_deref(), Some("1.19.2"));
    ctx
}

fn fixture(minimum: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("data");
    let source = temp.path().join("source");
    let plugin = source.join("needy");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(
        plugin.join("plugin.json"),
        json!({
            "id": "needy", "version": "1.0.0", "main": "plugin.js",
            "minCodeterm": minimum
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(plugin.join("plugin.js"), "module.exports = {};").unwrap();
    std::fs::write(
        source.join("channel.json"),
        json!({
            "schemaVersion": 1, "name": "fixture",
            "plugins": [{
                "id": "needy", "version": "1.0.0", "path": "needy",
                "minCodeterm": minimum
            }]
        })
        .to_string(),
    )
    .unwrap();
    let action = PluginAction::Channel {
        command: PluginChannelAction::Add {
            url: source.to_string_lossy().into_owned(),
            name: Some("fixture".into()),
        },
    };
    let ctx = resolve_with(root.clone(), &action, || panic!("local channel add")).unwrap();
    codeterm_plugins::run(ctx, convert_action(action).unwrap()).unwrap();
    (temp, root, plugin)
}

#[test]
fn running_host_version_gates_channel_and_directory_install() {
    for directory in [false, true] {
        for (minimum, compatible) in [("1.20.0", false), ("1.19.2", true)] {
            let (_temp, root, plugin) = fixture(minimum);
            let action = install(directory.then(|| plugin.to_string_lossy().into_owned()));
            let ctx = host_context(root.clone(), &action);
            let result = codeterm_plugins::run(ctx, convert_action(action).unwrap());
            if compatible {
                result.unwrap();
            } else {
                assert_eq!(
                    result.unwrap_err(),
                    "plugin 'needy' requires Domios \u{2265} 1.20.0, but this is 1.19.2"
                );
            }
            assert_eq!(root.join("plugins/needy/plugin.json").exists(), compatible);
        }
    }
}

#[test]
fn running_host_version_marks_channel_diff_incompatible() {
    let (_temp, root, _) = fixture("1.20.0");
    let action = PluginAction::Channel {
        command: PluginChannelAction::Diff {
            name: Some("fixture".into()),
        },
    };
    let ctx = host_context(root, &action);
    let result = codeterm_plugins::run(ctx, convert_action(action).unwrap()).unwrap();
    let rows = result["diff"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], "needy");
    assert_eq!(rows[0]["compatible"], false);
    assert_eq!(
        rows[0]["blocked_reason"],
        "requires Domios \u{2265} 1.20.0, but this is 1.19.2"
    );
}

#[test]
fn mutating_compatibility_actions_require_a_host_reported_version() {
    for action in [
        install(None),
        install(Some("source".into())),
        PluginAction::Update { id: None },
        PluginAction::UpdateSystem { id: None },
    ] {
        for identity in [json!({}), json!({"version": " "}), json!({"version": 1192})] {
            let error = resolve_with(PathBuf::new(), &action, || Ok(identity)).unwrap_err();
            assert_eq!(
                error,
                "system.identify did not report the running Domios version"
            );
        }
        let error =
            resolve_with(PathBuf::new(), &action, || Err("host unavailable".into())).unwrap_err();
        assert_eq!(error, "host unavailable");
    }
}

#[test]
fn offline_list_and_diff_do_not_claim_minimum_host_compatibility() {
    let (_temp, root, _) = fixture("1.20.0");
    codeterm_plugins::run(
        PluginCtx {
            data_root: root.clone(),
            version: Some("1.20.0".into()),
        },
        convert_action(install(None)).unwrap(),
    )
    .unwrap();
    for response in [
        Err("host unavailable".into()),
        Ok(json!({})),
        Ok(json!({"version": " "})),
    ] {
        let action = PluginAction::List;
        let ctx = resolve_with(root.clone(), &action, || response.clone()).unwrap();
        assert!(ctx.version.is_none());
        let result = codeterm_plugins::run(ctx, convert_action(action).unwrap()).unwrap();
        let row = result["plugins"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == "needy")
            .unwrap();
        assert_eq!(row["update"]["availability"]["compatible"], false);
        assert_eq!(row["update"]["reason"], "incompatible");

        let action = PluginAction::Channel {
            command: PluginChannelAction::Diff {
                name: Some("fixture".into()),
            },
        };
        let ctx = resolve_with(root.clone(), &action, || response).unwrap();
        let result = codeterm_plugins::run(ctx, convert_action(action).unwrap()).unwrap();
        let rows = result["diff"].as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["compatible"], false);
        assert_eq!(
            rows[0]["blocked_reason"],
            "requires Domios \u{2265} 1.20.0; running Domios version is unknown"
        );
    }
}
