use super::*;
use crate::provider::LaunchSpecParams;
use codeterm_types::agent::LaunchSpec;

pub(super) fn materialized_arguments(mut spec: LaunchSpec) -> LaunchSpec {
    for index in 1..spec.args.len() {
        let flag = spec.args[index - 1].as_str();
        if !matches!(flag, "--settings" | "--append-system-prompt-file") {
            continue;
        }
        let path = &spec.args[index];
        let contents = std::fs::read_to_string(path).unwrap();
        let value = if flag == "--settings" {
            serde_json::to_string(&serde_json::from_str::<serde_json::Value>(&contents).unwrap())
                .unwrap()
        } else {
            contents
        };
        std::fs::remove_file(path).unwrap();
        spec.args[index] = value;
    }
    spec
}

pub(super) fn assert_native_js_parity(native: &dyn AgentProvider, js: &dyn AgentProvider) {
    assert!(native.declares_launch_spec());
    assert_eq!(native.declares_launch_spec(), js.declares_launch_spec());
    for host_owned in [
        crate::provider::HostOwnedCapabilities::default(),
        crate::provider::HostOwnedCapabilities {
            questions: true,
            todos: false,
        },
        crate::provider::HostOwnedCapabilities {
            questions: true,
            todos: true,
        },
    ] {
        for skip_permissions in [false, true] {
            let resume = crate::provider::AgentResumeParams {
                session_id: "own-resume-session".into(),
                skip_permissions,
                host_owned,
                args: vec!["--model".into(), "fixture-model".into()],
                system_prompt: Some("opaque resume context 😀".into()),
                ..Default::default()
            };
            assert_eq!(
                materialized_arguments(
                    native
                        .launch_spec(&LaunchSpecParams::Resume(&resume))
                        .unwrap()
                ),
                materialized_arguments(js.launch_spec(&LaunchSpecParams::Resume(&resume)).unwrap()),
            );
            for (agent_name, session_id, args) in [
                (None, None, vec![]),
                (Some("agent 'юникод' 😀"), Some("fixture-session"), vec![]),
                (
                    None,
                    None,
                    vec!["--resume", "fixture-resume", "--model", "fixture-model"],
                ),
            ] {
                let params = AgentLaunchParams {
                    skip_permissions,
                    host_owned,
                    agent_name: agent_name.map(str::to_owned),
                    session_id: session_id.map(str::to_owned),
                    args: Some(args.into_iter().map(str::to_owned).collect()),
                    task: Some("opaque task 'quoted' $value 😀".into()),
                    ..Default::default()
                };
                assert_eq!(
                    materialized_arguments(
                        native
                            .launch_spec(&LaunchSpecParams::Fresh(&params))
                            .unwrap()
                    ),
                    materialized_arguments(
                        js.launch_spec(&LaunchSpecParams::Fresh(&params)).unwrap()
                    ),
                );
            }
        }
    }
}

fn fixture(spec: serde_json::Value, source: &str) -> JsProvider {
    let manifest_json = serde_json::json!({
        "id": "launch-spec-fixture", "version": "0.1.0", "displayName": "Fixture",
        "commands": {"autoApproveFlags": ["--approve", "value with spaces"], "launchSpec": spec}
    })
    .to_string();
    let manifest = serde_json::from_str(&manifest_json).unwrap();
    let vm = super::super::PluginVm::from_source_with(
        "launch-spec-fixture",
        source.into(),
        Some(manifest_json),
    )
    .unwrap();
    JsProvider::new(manifest, std::sync::Arc::new(vm))
}

#[test]
fn launch_spec_templates_expand_vectors_without_shell_parsing() {
    let js = fixture(
        serde_json::json!({
            "program": "fixture", "args": ["{skipPermissions}", "{args}", "{task}", "{sessionId}"],
            "envSet": {"VALUE": "{task}"}, "envUnset": ["INHERITED"]
        }),
        "module.exports.default = {};",
    );
    let params = AgentLaunchParams {
        args: Some(vec!["a b".into(), "$x; `command`".into()]),
        task: Some("юникод 😀 {args}".into()),
        session_id: Some("sid".into()),
        skip_permissions: true,
        ..Default::default()
    };
    let spec = js.launch_spec(&LaunchSpecParams::Fresh(&params)).unwrap();
    assert_eq!(
        spec.args,
        [
            "--approve",
            "value with spaces",
            "a b",
            "$x; `command`",
            "юникод 😀 {args}",
            "sid"
        ]
    );
    assert_eq!(spec.env_set["VALUE"], params.task.clone().unwrap());
    assert_eq!(spec.env_unset, ["INHERITED"]);
    let bare = js
        .launch_spec(&LaunchSpecParams::Fresh(&AgentLaunchParams::default()))
        .unwrap();
    assert_eq!(bare.args, ["", ""]);
}

#[test]
fn launch_spec_export_wins_and_absence_stays_on_shell_host() {
    let template = serde_json::json!({"program": "template", "args": []});
    let exported = fixture(
        template.clone(),
        r#"module.exports.default = {
        buildLaunchSpec() { return {program: "export", args: ["a b"], envUnset: ["X"]}; }
    };"#,
    );
    assert_eq!(
        exported
            .launch_spec(&LaunchSpecParams::Fresh(&Default::default()))
            .unwrap()
            .program,
        "export"
    );
    let refused = fixture(
        template,
        "module.exports.default = {buildLaunchSpec() { return null; }};",
    );
    assert!(refused
        .launch_spec(&LaunchSpecParams::Fresh(&Default::default()))
        .is_none());
    let absent = load_inline_js("absent-spec", "module.exports.default = {};");
    assert!(absent
        .launch_spec(&LaunchSpecParams::Fresh(&Default::default()))
        .is_none());
    for args in [
        serde_json::json!(["prefix{args}"]),
        serde_json::json!(["{unknown}"]),
    ] {
        assert!(fixture(
            serde_json::json!({"program": "fixture", "args": args}),
            "module.exports.default = {};"
        )
        .launch_spec(&LaunchSpecParams::Fresh(&Default::default()))
        .is_none());
    }
}

#[test]
fn launch_spec_declaration_tracks_manifest_and_export_reload() {
    let template = fixture(
        serde_json::json!({"program": "fixture", "args": []}),
        "module.exports.default = {};",
    );
    assert!(template.declares_launch_spec());
    let js = load_inline_js("declaration", "module.exports.default = {};");
    assert!(!js.declares_launch_spec());
    js.vm()
        .reload(
            "module.exports.default = {buildLaunchSpec() { throw Error('not queried'); }};".into(),
        )
        .unwrap();
    assert!(js.declares_launch_spec());
    js.vm()
        .reload("module.exports.default = {};".into())
        .unwrap();
    assert!(!js.declares_launch_spec());
}
