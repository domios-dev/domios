use super::*;

#[test]
fn set_scalar_replaces_value_preserving_comment() {
    let yaml = "# the speech engine\nengine: whisper # default\nport: 8080\n";
    let out = yaml_set_value(yaml, "engine", &infer_scalar("deepgram")).unwrap();
    assert!(out.contains("# the speech engine"), "leading comment kept");
    assert!(
        out.contains("engine: deepgram # default"),
        "inline comment kept: {out}"
    );
    assert!(out.contains("port: 8080"), "other lines kept");
}

#[test]
fn set_scalar_appends_unknown_key() {
    let out = yaml_set_value("engine: whisper\n", "serverUrl", &infer_scalar("http://x")).unwrap();
    assert!(out.contains("engine: whisper"));
    assert!(out.contains("serverUrl: http://x"));
}

#[test]
fn set_bool_writes_native_yaml_bool_not_string() {
    // `--set enabled=false` must write a real boolean, not the truthy string "false".
    let out = yaml_set_value("enabled: 'true'\n", "enabled", &infer_scalar("false")).unwrap();
    assert!(
        out.contains("enabled: false"),
        "native bool expected: {out}"
    );
    assert!(
        !out.contains("'false'") && !out.contains("\"false\""),
        "must not be a quoted string: {out}"
    );
}

fn endpoints_fixture() -> Value {
    serde_json::json!([
        {
            "name": "MiMo",
            "kind": "openai",
            "apiBase": "https://example.test/v1",
            "models": [{"id": "openai/mimo", "mapTokens": 8192}],
            "notes": "line one\nline two"
        },
        {"name": "Local", "kind": "openai", "models": []}
    ])
}

#[test]
fn array_of_objects_round_trips_through_yaml_parser() {
    let yaml = "# Aider settings\nplainMode: false # bypass adapter\n";
    let endpoints = endpoints_fixture();
    let out = yaml_set_value(yaml, "endpoints", &endpoints).unwrap();
    let parsed = parse_yaml_mapping(&out).expect("valid YAML");
    assert_eq!(parsed["endpoints"], endpoints, "{out}");
    assert_eq!(parsed["plainMode"], false);
    assert!(out.contains("# Aider settings"));
    assert!(out.contains("plainMode: false # bypass adapter"));
    assert!(!out.contains("endpoints: -"), "{out}");
}

#[test]
fn replacing_array_key_leaves_no_orphan_lines() {
    let yaml = "endpoints: # model endpoints\n  - name: Old\n    apiBase: https://old.test\n\n  - name: Older\n- name: ColumnZero\n\n# path doc\nconfigPath: /tmp/aider.yml\n";
    let endpoints = serde_json::json!([{"name": "New", "models": ["a"]}]);
    let out = yaml_set_value(yaml, "endpoints", &endpoints).unwrap();
    let parsed = parse_yaml_mapping(&out).expect("valid YAML");
    assert_eq!(parsed["endpoints"], endpoints, "{out}");
    assert_eq!(parsed["configPath"], "/tmp/aider.yml");
    assert_eq!(parsed.len(), 2, "{out}");
    for stale in ["Old", "old.test", "ColumnZero"] {
        assert!(!out.contains(stale), "orphan '{stale}' left: {out}");
    }
    assert!(out.contains("endpoints: # model endpoints"), "{out}");
    assert!(out.contains("\n\n# path doc\nconfigPath"), "{out}");
}

#[test]
fn rewriting_a_broken_single_line_array_repairs_the_document() {
    let broken = "endpoints: - apiBase: https://old.test\n  kind: openai\nplainMode: true\n";
    assert!(parse_yaml_mapping(broken).is_err());
    let endpoints = endpoints_fixture();
    let out = yaml_set_value(broken, "endpoints", &endpoints).unwrap();
    let parsed = parse_yaml_mapping(&out).expect("valid YAML");
    assert_eq!(parsed["endpoints"], endpoints);
    assert_eq!(parsed["plainMode"], true);
}

#[test]
fn replacing_array_with_scalar_drops_the_old_block() {
    let yaml = "endpoints:\n  - a\n  - b\nport: 1\n";
    let out = yaml_set_value(yaml, "endpoints", &serde_json::json!([])).unwrap();
    assert_eq!(out, "endpoints: []\nport: 1\n");
}

#[test]
fn scalar_edit_leaves_nested_blocks_untouched() {
    let yaml = "decision:\n  model: x # pick\n  maxTokens: 64\nbaseUrl: http://localhost:1234\n";
    let out = yaml_set_value(yaml, "baseUrl", &infer_scalar("http://x")).unwrap();
    assert_eq!(
        out,
        "decision:\n  model: x # pick\n  maxTokens: 64\nbaseUrl: http://x\n"
    );
}

#[test]
fn splice_that_cannot_match_falls_back_to_serializing_the_document() {
    let yaml = "\"endpoints\": [1]\nport: 2\n";
    let endpoints = serde_json::json!([{"name": "x"}]);
    let out = yaml_set_value(yaml, "endpoints", &endpoints).unwrap();
    let parsed = parse_yaml_mapping(&out).expect("valid YAML");
    assert_eq!(parsed["endpoints"], endpoints);
    assert_eq!(parsed["port"], 2);
    assert_eq!(parsed.len(), 2);
}

#[test]
fn unparseable_unrelated_yaml_is_refused_not_compounded() {
    let err = yaml_set_value("other: [\n", "port", &serde_json::json!(1)).unwrap_err();
    assert!(err.contains("not valid YAML"), "{err}");
}

#[test]
fn values_json_config_writes_yaml_the_host_parses() {
    let root = tempfile::tempdir().unwrap();
    let id = "aider-fixture";
    let dir = root.path().join("plugins").join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("plugin.json"),
        serde_json::json!({"id": id}).to_string(),
    )
    .unwrap();
    fs::write(dir.join("config.yaml"), "# docs\nplainMode: false\n").unwrap();
    let ctx = PluginCtx {
        data_root: root.path().to_path_buf(),
        version: None,
    };
    let endpoints = endpoints_fixture();
    let values = serde_json::json!({"endpoints": endpoints, "configPath": "/a b"});
    for _ in 0..2 {
        crate::run(
            ctx.clone(),
            PluginAction::Config {
                id: id.into(),
                set: Vec::new(),
                values_json: Some(values.to_string()),
                secret: None,
                grant: Vec::new(),
                revoke: Vec::new(),
            },
        )
        .unwrap();
    }
    let text = fs::read_to_string(dir.join("config.yaml")).unwrap();
    let parsed = parse_yaml_mapping(&text).expect("valid YAML");
    assert_eq!(parsed["endpoints"], endpoints, "{text}");
    assert_eq!(parsed["configPath"], "/a b");
    assert_eq!(parsed["plainMode"], false);
    assert!(text.starts_with("# docs\n"));
}

#[test]
fn infer_scalar_types_bool_number_string() {
    assert_eq!(infer_scalar("true"), serde_json::json!(true));
    assert_eq!(infer_scalar("false"), serde_json::json!(false));
    assert_eq!(infer_scalar("8080"), serde_json::json!(8080));
    assert_eq!(infer_scalar("0.5"), serde_json::json!(0.5));
    assert_eq!(infer_scalar("whisper"), serde_json::json!("whisper"));
    // A version-like / url string stays a string (not a number).
    assert_eq!(infer_scalar("http://x"), serde_json::json!("http://x"));
}

#[test]
fn redact_only_secret_fields_keeps_comments() {
    let yaml = "engine: whisper\napiKey: sk-secret # paste here\n";
    let out = redact_yaml(yaml, &["apiKey".to_string()]);
    assert!(out.contains("engine: whisper"));
    assert!(!out.contains("sk-secret"), "secret value must be gone");
    assert!(
        out.contains("apiKey: \"***\" # paste here"),
        "redacted + comment: {out}"
    );
}

#[test]
fn yaml_has_content_ignores_comments_and_blanks() {
    assert!(!yaml_has_content("# just a comment\n\n"));
    assert!(yaml_has_content("engine: whisper\n"));
}

fn temp_dir() -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("ct-grants-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn apply_grants_writes_and_merges_grants_yaml() {
    let dir = temp_dir();
    // Grant one, revoke another → grants.yaml the host reads via apply_persisted.
    let changed = apply_grants(
        &dir,
        &["spawn_agents".to_string()],
        &["manage_workspaces".to_string()],
    )
    .unwrap();
    assert!(changed.contains(&"spawn_agents".to_string()));
    let text = std::fs::read_to_string(dir.join("grants.yaml")).unwrap();
    let map: std::collections::BTreeMap<String, bool> = serde_yaml::from_str(&text).unwrap();
    assert_eq!(map.get("spawn_agents"), Some(&true));
    assert_eq!(map.get("manage_workspaces"), Some(&false));

    // A later grant merges (doesn't clobber the prior decision).
    apply_grants(&dir, &["agent_commands".to_string()], &[]).unwrap();
    let map2: std::collections::BTreeMap<String, bool> =
        serde_yaml::from_str(&std::fs::read_to_string(dir.join("grants.yaml")).unwrap()).unwrap();
    assert_eq!(map2.get("spawn_agents"), Some(&true));
    assert_eq!(map2.get("agent_commands"), Some(&true));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn project_write_grant_snapshots_configured_roots_and_revokes() {
    let dir = temp_dir();
    let project = temp_dir();
    std::fs::write(
        dir.join("plugin.json"),
        r#"{"permissions":{"projectFiles":["/root"]}}"#,
    )
    .unwrap();
    std::fs::write(
        dir.join("config.yaml"),
        serde_yaml::to_string(&serde_json::json!({"root":project})).unwrap(),
    )
    .unwrap();
    apply_grants(&dir, &["project_write".into()], &[]).unwrap();
    let granted = parse_grants(&std::fs::read_to_string(dir.join("grants.yaml")).unwrap()).unwrap();
    assert_eq!(
        granted.project_write_roots,
        [project.canonicalize().unwrap().to_string_lossy()]
    );
    assert_eq!(granted.permissions.get("project_write"), Some(&true));
    apply_grants(&dir, &[], &["project_write".into()]).unwrap();
    assert!(
        parse_grants(&std::fs::read_to_string(dir.join("grants.yaml")).unwrap())
            .unwrap()
            .project_write_roots
            .is_empty()
    );
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(project);
}

#[test]
fn apply_grants_refuses_to_replace_an_unparseable_grants_file() {
    let dir = temp_dir();
    let broken = "manage_workspaces: false
spawn_agents: [
";
    std::fs::write(dir.join("grants.yaml"), broken).unwrap();

    assert!(apply_grants(&dir, &["agent_commands".to_string()], &[]).is_err());

    assert!(!dir.join("grants.yaml").exists());
    let copies = grants_file(&dir).corrupt_copies();
    assert_eq!(std::fs::read_to_string(&copies[0]).unwrap(), broken);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn apply_grants_rejects_unknown_permission() {
    let dir = temp_dir();
    let err = apply_grants(&dir, &["take_over_the_world".to_string()], &[]).unwrap_err();
    assert!(err.contains("not a grantable permission"), "{err}");
    assert!(
        !dir.join("grants.yaml").exists(),
        "a rejected grant must not write a file"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn collect_secret_keys_finds_api_key_in_nested_section() {
    let schema = serde_json::json!([
        {"kind":"section","title":"Auth","fields":[
            {"kind":"api_key","env_var":"OPENAI_API_KEY","label":"Key"},
            {"kind":"text","key":"serverUrl","label":"URL"}
        ]}
    ]);
    let mut secret = Vec::new();
    collect_secret_keys(&schema, &mut secret);
    assert_eq!(secret, vec!["OPENAI_API_KEY".to_string()]);
    let mut all = Vec::new();
    collect_all_keys(&schema, &mut all);
    assert!(all.contains(&"serverUrl".to_string()));
    assert!(all.contains(&"OPENAI_API_KEY".to_string()));
}

#[test]
fn secret_config_uses_plugin_bucket_and_reports_only_presence() {
    let root = tempfile::tempdir().unwrap();
    let id = "secret-fixture";
    let dir = root.path().join("plugins").join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("plugin.json"),
        serde_json::json!({
            "id": id,
            "settingsSchema": "settings.schema.json"
        })
        .to_string(),
    )
    .unwrap();
    fs::write(
        dir.join("settings.schema.json"),
        serde_json::json!([{
            "kind": "api_key",
            "env_var": "SERVICE_TOKEN",
            "label": "Service token"
        }])
        .to_string(),
    )
    .unwrap();
    let ctx = PluginCtx {
        data_root: root.path().to_path_buf(),
        version: None,
    };
    let unset = crate::run(ctx.clone(), PluginAction::Get { id: id.into() }).unwrap();
    assert_eq!(unset["secret_status"]["SERVICE_TOKEN"], "unset");

    let value = format!("fixture-{}", uuid::Uuid::new_v4());
    let mut stdin_value = value.as_bytes();
    let secret_input = PluginSecretValue::read_from(&mut stdin_value, false).unwrap();
    assert!(!format!("{secret_input:?}").contains(value.as_str()));
    let configured = crate::run(
        ctx.clone(),
        PluginAction::Config {
            id: id.into(),
            set: Vec::new(),
            values_json: None,
            secret: Some(("SERVICE_TOKEN".into(), secret_input)),
            grant: Vec::new(),
            revoke: Vec::new(),
        },
    )
    .unwrap();
    assert_eq!(configured["secret_fields_set"][0], "SERVICE_TOKEN");
    assert!(!configured.to_string().contains(value.as_str()));
    let host_bucket = codeterm_util::secret_file::plugin_secret_bucket(root.path(), id);
    let stored = host_bucket.get("SERVICE_TOKEN").unwrap();
    assert!(stored.as_str() == value.as_str());
    let other_bucket = codeterm_util::secret_file::plugin_secret_bucket(root.path(), "other");
    assert!(other_bucket.get("SERVICE_TOKEN").is_err());

    let shown = crate::run(ctx.clone(), PluginAction::Get { id: id.into() }).unwrap();
    assert_eq!(shown["secret_status"]["SERVICE_TOKEN"], "set");
    assert!(!shown.to_string().contains(value.as_str()));
    assert!(!dir.join("config.yaml").exists());

    let set_error = crate::run(
        ctx,
        PluginAction::Config {
            id: id.into(),
            set: vec![format!("SERVICE_TOKEN={stored}")],
            values_json: None,
            secret: None,
            grant: Vec::new(),
            revoke: Vec::new(),
        },
    )
    .unwrap_err();
    assert!(set_error.contains("--secret SERVICE_TOKEN"));
    assert!(!set_error.contains(stored.as_str()));
    let audit = fs::read_to_string(root.path().join("logs/plugin-events.log")).unwrap();
    assert!(!audit.contains(stored.as_str()));
}

#[test]
fn get_returns_the_declared_verbs_and_bundled_agent_guide() {
    let root = tempfile::tempdir().unwrap();
    let id = "guide-fixture";
    let dir = root.path().join("plugins").join(id);
    fs::create_dir_all(dir.join("docs")).unwrap();
    fs::write(
        dir.join("plugin.json"),
        serde_json::json!({
            "id": id,
            "agentVerbs": ["zz-alpha"],
            "agentGuide": "docs/AGENT.md"
        })
        .to_string(),
    )
    .unwrap();
    fs::write(dir.join("docs/AGENT.md"), "guide-token-q7").unwrap();
    let ctx = PluginCtx {
        data_root: root.path().to_path_buf(),
        version: None,
    };
    let shown = crate::run(ctx, PluginAction::Get { id: id.into() }).unwrap();
    assert_eq!(shown["agent_verbs"], serde_json::json!(["zz-alpha"]));
    assert_eq!(shown["agent_guide"]["text"], "guide-token-q7");
}

#[test]
fn settings_persists_only_validated_exact_host_grants() {
    let root = temp_dir();
    let _ctx = enter_ctx(PluginCtx {
        data_root: root.clone(),
        version: None,
    });
    let id = PluginId::parse("local-model").unwrap();
    let dir = plugin_dir(&id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("grants.yaml"), "spawn_agents: true\n").unwrap();

    settings_command("local-model", &["API.Example.test:8443".into()], &[]).unwrap();
    let body = fs::read_to_string(dir.join("grants.yaml")).unwrap();
    let grants = parse_grants(&body).unwrap();
    assert_eq!(grants.permissions.get("spawn_agents"), Some(&true));
    assert_eq!(grants.network_allow, ["api.example.test:8443"]);

    assert!(settings_command("local-model", &["https://example.test".into()], &[]).is_err());
    assert!(settings_command("local-model", &["*.example.test".into()], &[]).is_err());
    let after_refusals = fs::read_to_string(dir.join("grants.yaml")).unwrap();
    assert_eq!(
        parse_grants(&after_refusals).unwrap().network_allow,
        ["api.example.test:8443"]
    );
    let _ = fs::remove_dir_all(root);
}
