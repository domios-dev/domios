use super::*;

const PROVIDER_ID: &str = "opencode";
const STORE_FILE: &str = "opencode.db";
const STORE_ENV: &str = "OPENCODE_DB";
const BASE_SCHEMA: &str =
    include_str!("../../../../../../plugins/provider-opencode/tests/fixtures/sqlite-1.18.35.sql");

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
    for (version, fixture, count) in [
        ("1.18.35", BASE_SCHEMA, 2),
        (
            "1.19.0",
            include_str!(
                "../../../../../../plugins/provider-opencode/tests/fixtures/sqlite-1.19.0.sql"
            ),
            3,
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join(STORE_FILE);
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.execute_batch(fixture).unwrap();
        drop(conn);
        let _env = EnvGuard::new(&[(STORE_ENV, db.to_str().unwrap())]);
        let provider =
            super::super::provider_observation::with_fixture_version(Some(version), || {
                load_js(PROVIDER_ID)
            });
        assert_eq!(
            provider
                .cli_compatibility()
                .unwrap()
                .evaluate(Some(version))
                .verdict,
            crate::provider_compat::CompatibilityVerdict::Supported
        );
        assert_eq!(
            provider.session_exists("/fixture", "ses_fixture_owned"),
            Some(true)
        );
        assert_eq!(
            provider.session_exists("/fixture", "ses_missing"),
            Some(false)
        );
        assert!(provider
            .build_resume_command("ses_fixture_owned", false)
            .unwrap()
            .contains("ses_fixture_owned"));
        let chat = provider.read_structured_chat("/fixture", "ses_fixture_owned", None);
        assert_eq!(chat.messages.len(), count);
        assert!(chat
            .messages
            .iter()
            .any(|message| message.kind == "user" && message.content == "opaque-user-turn"));
        assert!(chat.messages.iter().any(
            |message| message.kind == "assistant" && message.content == "opaque-assistant-turn"
        ));
        for version in [None, Some("2.0.0")] {
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
        for version in ["1.18.36", "1.20.0", "3.0.0"] {
            let unverified =
                super::super::provider_observation::with_fixture_version(Some(version), || {
                    load_js("opencode")
                });
            assert_eq!(
                unverified
                    .cli_compatibility()
                    .unwrap()
                    .evaluate(Some(version))
                    .verdict,
                crate::provider_compat::CompatibilityVerdict::Unknown
            );
            assert_eq!(
                unverified.session_exists("/fixture", "ses_fixture_owned"),
                Some(true)
            );
            assert_eq!(
                unverified
                    .read_structured_chat("/fixture", "ses_fixture_owned", None)
                    .messages,
                chat.messages
            );
        }
    }
}

#[test]
fn structured_chat_normalizes_system_frame_from_user_record() {
    let fixtures: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../../tests/fixtures/message-envelope.json"
    ))
    .unwrap();
    let fixture = fixtures
        .as_array()
        .unwrap()
        .iter()
        .find(|fixture| fixture["name"] == "system header with trailing Windows path")
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join(STORE_FILE);
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch(BASE_SCHEMA).unwrap();
    let part = serde_json::json!({ "type": "text", "text": fixture["raw"] });
    conn.execute(
        "UPDATE part SET data = ?1 WHERE message_id = 'msg_fixture_user'",
        [part.to_string()],
    )
    .unwrap();
    drop(conn);
    let _env = EnvGuard::new(&[(STORE_ENV, db.to_str().unwrap())]);
    let provider = load_sqlite_fixture_js(PROVIDER_ID);
    assert!(provider.uses_structured_chat());
    let chat = provider.read_structured_chat("/fixture", "ses_fixture_owned", None);
    let message = chat
        .messages
        .iter()
        .find(|message| message.kind == "user")
        .unwrap();
    assert_eq!(message.content, fixture["raw"].as_str().unwrap());
    assert_eq!(
        serde_json::to_value(&message.segments).unwrap(),
        fixture["segments"]
    );
}

#[test]
fn opencode_unverified_sqlite_changed_shape_returns_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("opencode.db");
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch(
        "CREATE TABLE session (replacement_id TEXT); CREATE TABLE part (replacement_data TEXT);",
    )
    .unwrap();
    drop(conn);
    let _env = EnvGuard::new(&[("OPENCODE_DB", db.to_str().unwrap())]);
    let provider = super::super::provider_observation::with_fixture_version(Some("1.20.0"), || {
        load_js("opencode")
    });
    assert_eq!(
        provider.session_exists("/fixture", "ses_fixture_owned"),
        None
    );
    assert!(provider
        .read_structured_chat("/fixture", "ses_fixture_owned", None)
        .messages
        .is_empty());
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
