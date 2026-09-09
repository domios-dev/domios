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
            "freshContext": "/new",
            "exit": "/exit",
            "autoApproveFlags": ["--yolo"]
        }
    }"#;

#[test]
fn session_ownership_uses_only_provider_declared_records() {
    let dir = tempfile::tempdir().unwrap();
    let owned = dir.path().join("owned.jsonl");
    std::fs::write(&owned, "{}").unwrap();
    let path_json = serde_json::to_string(&owned.to_string_lossy()).unwrap();
    let script = format!(
        "module.exports.default = {{ sessionFilePath(cwd, id) {{ return id === 'owned' ? {path_json} : null; }} }};"
    );
    let provider = load(MIN_MANIFEST, &script);
    assert!(provider.session_exists("/work", "owned"));
    assert!(!provider.session_exists("/work", "foreign"));
    let undeclared = load(MIN_MANIFEST, "module.exports.default = {};");
    assert!(!undeclared.session_exists("/work", "owned"));
    let indexed = load(
        MIN_MANIFEST,
        "module.exports.default = { sessionExists(cwd, id) { return id === 'indexed'; }, sessionFilePath() { return null; } };",
    );
    assert!(indexed.session_exists("/work", "indexed"));
    assert!(!indexed.session_exists("/work", "foreign"));
}

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
    assert_eq!(p.fresh_context_command(), Some("/new"));
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
fn session_jsonl_path_accepts_plugin_null() {
    let js = r#"
            module.exports.default = {
                sessionJsonlPath() { return null; }
            };
        "#;
    let p = load(MIN_MANIFEST, js);
    assert_eq!(p.session_jsonl_path("/work", "pending-session"), None);
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
    assert_eq!(p.busy_submit_strategy(), BusySubmitStrategy::SubmitNormally);
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

/// The session file is streamed to the plugin in line-aligned chunks instead of
/// one `read_to_string`. A fixture larger than `SESSION_PARSE_CHUNK_BYTES` forces
/// more than one QuickJS call; every line must still arrive exactly once, in
/// order, and no line may be split across a chunk boundary.
#[test]
fn parse_session_messages_streams_multi_chunk_files_without_splitting_lines() {
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
    // Each line is ~1 KiB, so 6000 lines clears the 4 MiB chunk budget twice.
    let filler = "x".repeat(1000);
    let mut body = String::new();
    for i in 0..6000 {
        body.push_str(&format!("line-{i}-{filler}\n"));
    }
    assert!(body.len() > 4 * 1024 * 1024, "fixture must span >1 chunk");
    std::fs::write(tmp.path(), &body).unwrap();

    let msgs = p.parse_session_messages(tmp.path());
    assert_eq!(
        msgs.len(),
        6000,
        "every line survives chunking exactly once"
    );
    assert_eq!(msgs[0].text, format!("line-0-{filler}"));
    assert_eq!(msgs[5999].text, format!("line-5999-{filler}"));
    for (i, m) in msgs.iter().enumerate() {
        assert!(
            m.text.starts_with(&format!("line-{i}-")),
            "line {i} was reordered or split: {}",
            &m.text[..m.text.len().min(24)]
        );
    }
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
    let prompt = p.parse_prompt("anything").unwrap();
    assert_eq!(prompt.question, "Allow?");
    assert_eq!(
        prompt
            .options
            .iter()
            .map(|o| (o.label.clone(), o.value.clone()))
            .collect::<Vec<_>>(),
        vec![("y".into(), "Yes".into()), ("n".into(), "No".into())]
    );
}

#[test]
fn real_opencode_plugin_parses_permission_box() {
    let js = include_str!("../../../../../../plugins/provider-opencode/plugin.js");
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
    let prompt = p.parse_prompt(&box_text).expect("permission box parses");
    assert_eq!(prompt.question, "Access external directory /tmp");
    assert_eq!(
        prompt
            .options
            .iter()
            .map(|o| (o.label.clone(), o.value.clone()))
            .collect::<Vec<_>>(),
        vec![
            ("Allow once".into(), "1".into()),
            ("Allow always".into(), "2".into()),
            ("Reject".into(), "3".into()),
        ]
    );
    assert!(p.parse_prompt("just regular output line").is_none());
}

#[test]
fn build_prompt_response_decodes_base64_steps() {
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
    let steps = p.build_prompt_response(2, "");
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
    assert_eq!(s.len(), 2);
}

#[test]
fn available_models_reads_catalog_snapshot_not_static_manifest() {
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
    crate::model_catalog::put_live(
        "am",
        vec![crate::provider::ModelInfo {
            id: "live".into(),
            display_name: "Live".into(),
            description: Some("from catalog".into()),
            available: None,
            availability: None,
        }],
        Some("live".into()),
    );

    let ms = p.available_models();
    assert_eq!(ms.len(), 1);
    assert_eq!(ms[0].id, "live");
    assert_eq!(ms[0].description.as_deref(), Some("from catalog"));
}

#[test]
fn settings_model_options_follow_catalog_changes() {
    let json = r#"{
            "id": "dynamic-settings-models",
            "version": "0.1.0",
            "displayName": "Dynamic Settings Models",
            "spawn": {
                "readinessStrategy": { "kind": "fixed_delay", "durationMs": 0 },
                "taskDelivery": "pty_input",
                "verifySubmit": { "kind": "none" }
            },
            "models": [
                { "id": "seed", "displayName": "Seed" }
            ],
            "defaultModel": "seed"
        }"#;
    let p = load(json, "module.exports.default = {};");

    crate::model_catalog::put_live(
        "dynamic-settings-models",
        vec![crate::provider::ModelInfo {
            id: "live".into(),
            display_name: "Live".into(),
            description: None,
            available: None,
            availability: None,
        }],
        Some("live".into()),
    );

    let settings = p.settings_schema();
    let SettingField::Section { fields, .. } = &settings[0] else {
        panic!("model section expected");
    };
    let SettingField::Select {
        options, default, ..
    } = &fields[0]
    else {
        panic!("model selector expected");
    };
    assert_eq!(options, &vec![("live".into(), "Live".into())]);
    assert_eq!(default.as_deref(), Some("live"));
}

#[tokio::test]
async fn discover_models_delegates_to_optional_js_export() {
    let js = r#"
            module.exports.default = {
                discoverModels() {
                    return [
                        { id: 'live-a', displayName: 'Live A' },
                        { id: 'live-b', displayName: 'Live B', description: 'second' }
                    ];
                }
            };
        "#;
    let p = load(MIN_MANIFEST, js);
    let ms = p.discover_models().await.expect("discoverModels result");
    assert_eq!(ms.len(), 2);
    assert_eq!(ms[0].id, "live-a");
    assert_eq!(ms[0].display_name, "Live A");
    assert_eq!(ms[1].description.as_deref(), Some("second"));
}

#[tokio::test]
async fn discover_models_missing_export_returns_none() {
    let p = load(MIN_MANIFEST, "module.exports.default = {};");
    assert_eq!(p.discover_models().await, None);
}

#[tokio::test]
async fn discover_models_null_return_is_no_discovery() {
    let p = load(
        MIN_MANIFEST,
        "module.exports.default = { discoverModels() { return null; } };",
    );
    assert_eq!(
        p.discover_models().await,
        None,
        "a JS null means no genuine discovery — never a fallback list"
    );
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
fn update_command_resolved_per_platform() {
    let json = r#"{
            "id": "uc",
            "version": "0.1.0",
            "displayName": "UC",
            "spawn": {
                "readinessStrategy": { "kind": "fixed_delay", "durationMs": 0 },
                "taskDelivery": "pty_input",
                "verifySubmit": { "kind": "none" }
            },
            "updater": {
                "macos": "opaque-update-macos",
                "windows": "opaque-update-windows"
            }
        }"#;
    let provider = load(json, "module.exports.default = {};");
    assert_eq!(
        provider.update_command("macos").as_deref(),
        Some("opaque-update-macos")
    );
    assert_eq!(
        provider.update_command("windows").as_deref(),
        Some("opaque-update-windows")
    );
    assert_eq!(provider.update_command("linux"), None);
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
fn generic_detect_session_id_refuses_to_guess_when_ambiguous() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().to_string_lossy().to_string();
    let json = format!(
        r#"{{
            "id": "ds",
            "version": "0.1.0",
            "displayName": "DS",
            "spawn": {{
                "readinessStrategy": {{ "kind": "fixed_delay", "durationMs": 0 }},
                "taskDelivery": "pty_input",
                "verifySubmit": {{ "kind": "none" }}
            }},
            "sessions": {{
                "rootDir": "{}",
                "cwdEncoding": "none",
                "fileExtension": "jsonl"
            }}
        }}"#,
        root.replace('\\', "\\\\")
    );
    let p = load(&json, "module.exports.default = {};");

    let cwd = "proj";
    let dir = tmp.path().join(cwd);
    std::fs::create_dir_all(&dir).unwrap();
    let s1 = "aaaa1111-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    let s2 = "bbbb2222-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
    std::fs::write(dir.join(format!("{s1}.jsonl")), "{}\n").unwrap();
    std::fs::write(dir.join(format!("{s2}.jsonl")), "{}\n").unwrap();

    assert_eq!(
        p.detect_session_id(cwd, &[], Some(Duration::from_secs(300))),
        None,
        "must refuse to guess when 2+ sibling sessions are viable candidates",
    );

    assert_eq!(
        p.detect_session_id(cwd, &[s2.to_string()], Some(Duration::from_secs(300)))
            .as_deref(),
        Some(s1),
        "with the sibling excluded there is exactly one candidate → bind it",
    );
}

#[test]
fn _unused_import_guard() {
    let _ = encode_cwd_dashed("/home/x/proj");
    let _ = json!(null);
}
