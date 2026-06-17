use super::*;
use serde_json::json;

fn load(manifest_json: &str, plugin_js: &str) -> JsProvider {
    let mut m: PluginManifest = serde_json::from_str(manifest_json).expect("parse manifest");
    m.source_path = Some(PathBuf::from("/tmp/test-plugin/plugin.json"));
    let vm = PluginVm::from_source(&m.id, plugin_js.to_string()).expect("vm");
    JsProvider::new(m, Arc::new(vm))
}

const MIN_MANIFEST: &str = r#"{
        "id": "demo",
        "version": "0.1.0",
        "displayName": "Demo",
        "binaryName": "demo",
        "spawn": {
            "readinessStrategy": { "kind": "poll_shell_prompt", "timeoutSecs": 5 },
            "taskDelivery": "pty_input",
            "verifySubmit": { "kind": "none" },
            "statusBarRows": 2,
            "hasHooks": true
        },
        "commands": {
            "compact": "/compact",
            "exit": "/exit",
            "autoApproveFlags": ["--yolo"]
        }
    }"#;

#[test]
fn id_display_binary_from_manifest() {
    let p = load(MIN_MANIFEST, "module.exports.default = {};");
    assert_eq!(p.id(), "demo");
    assert_eq!(p.display_name(), "Demo");
    assert_eq!(p.binary_name(), "demo");
}

#[test]
fn binary_name_defaults_to_id_when_omitted() {
    let json = r#"{
            "id": "demo2",
            "version": "0.1.0",
            "displayName": "D2",
            "spawn": {
                "readinessStrategy": { "kind": "fixed_delay", "durationMs": 0 },
                "taskDelivery": "pty_input",
                "verifySubmit": { "kind": "none" }
            }
        }"#;
    let p = load(json, "module.exports.default = {};");
    assert_eq!(p.binary_name(), "demo2");
}

#[test]
fn commands_read_off_manifest() {
    let p = load(MIN_MANIFEST, "module.exports.default = {};");
    assert_eq!(p.compact_command(), Some("/compact"));
    assert_eq!(p.exit_command(), Some("/exit"));
    assert_eq!(p.auto_approve_flags(), vec!["--yolo".to_string()]);
    assert_eq!(p.status_bar_rows(), 2);
    assert!(p.has_hooks());
}

#[test]
fn model_switch_command_substitutes_template() {
    let json = r#"{
            "id": "m",
            "version": "0.1.0",
            "displayName": "M",
            "spawn": {
                "readinessStrategy": { "kind": "fixed_delay", "durationMs": 0 },
                "taskDelivery": "pty_input",
                "verifySubmit": { "kind": "none" }
            },
            "commands": {
                "modelSwitchFormat": "/model {model}"
            }
        }"#;
    let p = load(json, "module.exports.default = {};");
    assert_eq!(
        p.model_switch_command("opus").as_deref(),
        Some("/model opus")
    );
}

#[test]
fn detect_from_title_dispatches_to_js() {
    let js = r#"
            module.exports.default = {
                detectFromTitle(t) { return t.toLowerCase().includes('demo'); }
            };
        "#;
    let p = load(MIN_MANIFEST, js);
    assert!(p.detect_from_title("Running Demo CLI"));
    assert!(!p.detect_from_title("other"));
}

#[test]
fn missing_detect_returns_false() {
    let p = load(MIN_MANIFEST, "module.exports.default = {};");
    assert!(!p.detect_from_title("demo"));
    assert!(!p.detect_from_output("demo"));
}

#[test]
fn build_launch_command_passes_params_to_js() {
    let js = r#"
            module.exports.default = {
                buildLaunchCommand(p) {
                    let s = 'demo';
                    if (p.skipPermissions) s += ' --yolo';
                    if (p.task) s += ' --task ' + JSON.stringify(p.task);
                    if (p.sessionId) s += ' --session-id ' + p.sessionId;
                    return s;
                }
            };
        "#;
    let p = load(MIN_MANIFEST, js);
    let params = AgentLaunchParams {
        task: Some("hello world".into()),
        skip_permissions: true,
        session_id: Some("sid-1".into()),
        ..Default::default()
    };
    let cmd = p.build_launch_command(&params);
    assert_eq!(cmd, "demo --yolo --task \"hello world\" --session-id sid-1");
}

#[test]
fn readiness_and_verify_from_manifest_enums() {
    let p = load(MIN_MANIFEST, "module.exports.default = {};");
    match p.readiness_strategy() {
        ReadinessStrategy::PollShellPrompt { timeout } => {
            assert_eq!(timeout, Duration::from_secs(5));
        }
        _ => panic!(),
    }
    assert!(matches!(
        p.verify_submit_strategy(),
        VerifySubmitStrategy::None
    ));
}

#[test]
fn session_file_path_uses_manifest_root() {
    let json = r#"{
            "id": "s",
            "version": "0.1.0",
            "displayName": "S",
            "spawn": {
                "readinessStrategy": { "kind": "fixed_delay", "durationMs": 0 },
                "taskDelivery": "pty_input",
                "verifySubmit": { "kind": "none" }
            },
            "sessions": {
                "rootDir": "/tmp/s-sessions",
                "cwdEncoding": "dash_encoded",
                "fileExtension": "jsonl"
            }
        }"#;
    let p = load(json, "module.exports.default = {};");
    let path = p.session_file_path("/home/x/proj", "abc123").unwrap();
    let s = path.to_string_lossy();
    assert!(s.contains("-home-x-proj"));
    assert!(s.ends_with("abc123.jsonl"), "got {s}");
}

#[test]
fn parse_session_messages_delegates_to_js() {
    let js = r#"
            module.exports.default = {
                parseSessionMessages(text) {
                    return text.split('\n').filter(Boolean).map(l => ({
                        role: 'user', text: l, isMeta: false
                    }));
                }
            };
        "#;
    let p = load(MIN_MANIFEST, js);
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), "hello\nworld\n").unwrap();
    let msgs = p.parse_session_messages(tmp.path());
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0].text, "hello");
    assert_eq!(msgs[1].text, "world");
}

#[test]
fn extract_session_name_passes_through() {
    let js = r#"
            module.exports.default = {
                extractSessionName(screen) {
                    const m = /session: ([\w-]+)/.exec(screen);
                    return m ? m[1] : null;
                }
            };
        "#;
    let p = load(MIN_MANIFEST, js);
    assert_eq!(
        p.extract_session_name("session: vivid-bhabha-9").as_deref(),
        Some("vivid-bhabha-9")
    );
    assert_eq!(p.extract_session_name("nothing here"), None);
}

#[test]
fn parse_prompt_decodes_options() {
    let js = r#"
            module.exports.default = {
                parsePrompt(_) {
                    return { question: 'Allow?', options: [['y','Yes'],['n','No']] };
                }
            };
        "#;
    let p = load(MIN_MANIFEST, js);
    let (q, opts) = p.parse_prompt("anything").unwrap();
    assert_eq!(q, "Allow?");
    assert_eq!(
        opts,
        vec![("y".into(), "Yes".into()), ("n".into(), "No".into())]
    );
}

#[test]
fn real_opencode_plugin_parses_permission_box() {
    // Guards the shipped opencode plugin's own permission parser against
    // regressions — the plugin owns its format, the host just navigates.
    let js = include_str!("../../../../../plugins/provider-opencode/plugin.js");
    let p = load(MIN_MANIFEST, js);
    let box_text = [
        "╭──────────────────────────────────╮",
        "│ ⚠ Permission required            │",
        "│ Access external directory /tmp   │",
        "│ Patterns                         │",
        "│ - /tmp/*                         │",
        "│ Allow once   Allow always   Reject │",
        "╰──────────────────────────────────╯",
        "ctrl+f fullscreen  ⇅ select  enter confirm",
    ]
    .join("\n");
    let (q, opts) = p.parse_prompt(&box_text).expect("permission box parses");
    assert_eq!(q, "Access external directory /tmp");
    assert_eq!(
        opts,
        vec![
            ("Allow once".into(), "1".into()),
            ("Allow always".into(), "2".into()),
            ("Reject".into(), "3".into()),
        ]
    );
    // Plain output must not be misread as a prompt.
    assert!(p.parse_prompt("just regular output line").is_none());
}

#[test]
fn build_prompt_response_decodes_base64_steps() {
    // 'j' = ag==, '\r' = DQ==
    let js = r#"
            module.exports.default = {
                buildPromptResponse(idx) {
                    const steps = [];
                    for (let i = 0; i < idx; i++) steps.push('ag==');
                    steps.push('DQ==');
                    return steps;
                }
            };
        "#;
    let p = load(MIN_MANIFEST, js);
    let steps = p.build_prompt_response(2);
    assert_eq!(steps, vec![b"j".to_vec(), b"j".to_vec(), b"\r".to_vec()]);
}

#[test]
fn settings_schema_from_plugin_overrides_manifest_defaults() {
    let js = r#"
            module.exports.default = {
                settingsSchema() {
                    return [{ kind: 'section', title: 'Custom', fields: [] }];
                }
            };
        "#;
    let p = load(MIN_MANIFEST, js);
    let s = p.settings_schema();
    assert_eq!(s.len(), 1);
    match &s[0] {
        SettingField::Section { title, .. } => assert_eq!(title, "Custom"),
        _ => panic!(),
    }
}

#[test]
fn settings_schema_falls_back_to_manifest_when_plugin_silent() {
    let json = r#"{
            "id": "ms",
            "version": "0.1.0",
            "displayName": "MS",
            "spawn": {
                "readinessStrategy": { "kind": "fixed_delay", "durationMs": 0 },
                "taskDelivery": "pty_input",
                "verifySubmit": { "kind": "none" }
            },
            "models": [
                { "id": "m1", "displayName": "Model 1" }
            ],
            "defaultModel": "m1",
            "commands": {
                "autoApproveFlags": ["--yolo"]
            }
        }"#;
    let p = load(json, "module.exports.default = {};");
    let s = p.settings_schema();
    // session_resume + model_select expected
    assert_eq!(s.len(), 2);
}

#[test]
fn available_models_round_trip_through_manifest() {
    let json = r#"{
            "id": "am",
            "version": "0.1.0",
            "displayName": "AM",
            "spawn": {
                "readinessStrategy": { "kind": "fixed_delay", "durationMs": 0 },
                "taskDelivery": "pty_input",
                "verifySubmit": { "kind": "none" }
            },
            "models": [
                { "id": "a", "displayName": "A" },
                { "id": "b", "displayName": "B", "description": "second" }
            ]
        }"#;
    let p = load(json, "module.exports.default = {};");
    let ms = p.available_models();
    assert_eq!(ms.len(), 2);
    assert_eq!(ms[1].description.as_deref(), Some("second"));
}

#[test]
fn install_command_resolved_per_platform() {
    let json = r#"{
            "id": "ic",
            "version": "0.1.0",
            "displayName": "IC",
            "spawn": {
                "readinessStrategy": { "kind": "fixed_delay", "durationMs": 0 },
                "taskDelivery": "pty_input",
                "verifySubmit": { "kind": "none" }
            },
            "installer": {
                "macos": "brew install ic",
                "linux": "snap install ic"
            }
        }"#;
    let p = load(json, "module.exports.default = {};");
    assert_eq!(
        p.install_command("macos").as_deref(),
        Some("brew install ic")
    );
    assert_eq!(
        p.install_command("linux").as_deref(),
        Some("snap install ic")
    );
    assert_eq!(p.install_command("windows"), None);
}

#[test]
fn detect_events_returns_permission_request_via_plugin() {
    let js = r#"
            module.exports.default = {
                detectEvents(text) {
                    return text.includes('ALLOW?') ? ['permission_request'] : [];
                }
            };
        "#;
    let p = load(MIN_MANIFEST, js);
    assert_eq!(p.detect_events("plain text").len(), 0);
    let evts = p.detect_events("Do you ALLOW?");
    assert_eq!(evts.len(), 1);
}

#[test]
fn build_resume_command_returns_none_when_plugin_missing() {
    let p = load(MIN_MANIFEST, "module.exports.default = {};");
    assert_eq!(p.build_resume_command("sid", true), None);
}

#[test]
fn build_resume_command_dispatches_to_js_with_two_args() {
    let js = r#"
            module.exports.default = {
                buildResumeCommand(sid, skip) {
                    return 'demo --resume ' + sid + (skip ? ' --yolo' : '');
                }
            };
        "#;
    let p = load(MIN_MANIFEST, js);
    assert_eq!(
        p.build_resume_command("S1", true),
        Some("demo --resume S1 --yolo".into())
    );
    assert_eq!(
        p.build_resume_command("S1", false),
        Some("demo --resume S1".into())
    );
}

#[test]
fn _unused_import_guard() {
    // Keep the encode helper exercised so the test suite reflects its production use in sessions_dir().
    let _ = encode_cwd_dashed("/home/x/proj");
    let _ = json!(null);
}
