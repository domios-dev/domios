use super::*;

pub(super) fn load_sqlite_fixture_js(id: &str) -> JsProvider {
    let directory = workspace_plugins_root().join(format!("provider-{id}"));
    let mut json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(directory.join("plugin.json")).unwrap())
            .unwrap();
    json["cliCompatibility"]["supported"] = serde_json::json!(["=1.0.0"]);
    let json = json.to_string();
    let manifest = super::super::PluginManifest::from_json_str(&json).unwrap();
    let source = std::fs::read_to_string(directory.join("plugin.js")).unwrap();
    let vm = super::super::provider_observation::with_fixture_version(Some("1.0.0"), || {
        super::super::PluginVm::from_source_with(id, source, Some(json)).unwrap()
    });
    JsProvider::new(manifest, std::sync::Arc::new(vm))
}

#[test]
fn opencode_declared_sqlite_version_resumes_exact_session_with_history() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch(include_str!(
        "../../../../../../plugins/provider-opencode/tests/fixtures/sqlite-1.18.35.sql"
    ))
    .unwrap();
    drop(conn);
    let _env = EnvGuard::new(&[("OPENCODE_DB", db.to_str().unwrap())]);
    let provider =
        super::super::provider_observation::with_fixture_version(Some("1.18.35"), || {
            load_js("opencode")
        });
    assert_eq!(
        provider
            .cli_compatibility()
            .unwrap()
            .evaluate(Some("1.18.35"))
            .verdict,
        crate::provider_compat::CompatibilityVerdict::Supported
    );
    assert!(provider.session_exists("/fixture", "ses_fixture_owned") == Some(true));
    assert_eq!(
        provider.session_exists("/fixture", "ses_missing"),
        Some(false)
    );
    let command = provider
        .build_resume_command("ses_fixture_owned", false)
        .unwrap();
    assert!(command.contains("ses_fixture_owned"));
    let chat = provider.read_structured_chat("/fixture", "ses_fixture_owned", None);
    assert_eq!(
        chat.messages
            .iter()
            .map(|message| (message.kind.as_str(), message.content.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("user", "opaque-user-turn"),
            ("assistant", "opaque-assistant-turn")
        ]
    );
    for version in [None, Some("1.18.36"), Some("2.0.0")] {
        let blocked = super::super::provider_observation::with_fixture_version(version, || {
            load_js("opencode")
        });
        assert_eq!(
            blocked.session_exists("/fixture", "ses_fixture_owned"),
            None
        );
        assert!(blocked
            .read_structured_chat("/fixture", "ses_fixture_owned", None)
            .messages
            .is_empty());
    }
}

#[test]
fn opencode_unreadable_store_is_unknown_instead_of_absent() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    std::fs::write(&db, "unreadable-store-fixture").unwrap();
    let _env = EnvGuard::new(&[("OPENCODE_DB", db.to_str().unwrap())]);
    let provider = load_sqlite_fixture_js("opencode");
    assert_eq!(
        provider.session_exists("/fixture", "ses_fixture_owned"),
        None
    );
}
