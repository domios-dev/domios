use super::*;
use crate::provider_account::{with_context, InvocationContext};
use std::collections::BTreeMap;
use std::sync::Arc;

fn selected(
    root: &std::path::Path,
    account: &str,
    shared: &std::path::Path,
) -> Arc<InvocationContext> {
    let home = root.join(account);
    InvocationContext::new(
        "claude".into(),
        account.into(),
        BTreeMap::from([
            (
                "CLAUDE_CONFIG_DIR".into(),
                Some(home.to_string_lossy().into_owned()),
            ),
            (
                "FIXTURE_ACCOUNT_HOME".into(),
                Some(home.to_string_lossy().into_owned()),
            ),
            (
                "FIXTURE_SHARED_HISTORY".into(),
                Some(shared.to_string_lossy().into_owned()),
            ),
            ("ANTHROPIC_API_KEY".into(), None),
        ]),
        vec![home, shared.into()],
        vec![root.into()],
    )
    .unwrap()
}

#[test]
fn parity_account_home_credentials_jobs_and_shared_history_use_one_invocation() {
    let root = tempfile::tempdir().unwrap();
    let shared = root.path().join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    std::fs::write(shared.join("history.jsonl"), "shared-history").unwrap();
    for account in ["a", "b"] {
        let home = root.path().join(account);
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            home.join("auth.json"),
            serde_json::json!({"account":account,"secret":format!("secret-{account}")}).to_string(),
        )
        .unwrap();
        std::fs::write(home.join("models.json"), account).unwrap();
    }
    std::fs::write(
        root.path().join("usual-auth.json"),
        r#"{"account":"usual","secret":"usual-secret"}"#,
    )
    .unwrap();
    let manifest = serde_json::json!({"id":"claude","version":"0.1.0","displayName":"Fixture","hostIoJobs":true,
        "credentials":[{"id":"auth","file":root.path().join("usual-auth.json"),"fileEnvironment":{"variable":"FIXTURE_ACCOUNT_HOME","subdir":"auth.json"},"public":{"account":"account"},"secret":{"token":"secret"}}]}).to_string();
    let declaration = super::super::PluginManifest::from_json_str(&manifest).unwrap();
    let sandbox = Arc::new(
        super::super::host_api::PluginSandbox::unrestricted()
            .with_identity("claude")
            .with_credentials(&declaration.credentials),
    );
    let source = r#"
        module.exports.default = {
            probe(options) { return host.io.run(host.io.exec(options)); },
            fetchUsage() { return host.io.run((function* () {
                const own = host.envGet('FIXTURE_ACCOUNT_HOME');
                const shared = host.envGet('FIXTURE_SHARED_HISTORY');
                const account = yield {op:'credentialPublic',id:'auth'};
                const models = yield* host.io.fs.readFile(own + '/models.json');
                const history = yield* host.io.fs.readFile(shared + '/history.jsonl');
                const secret = yield* host.io.fs.readFile(own + '/auth.json');
                return JSON.stringify({account, models, history, secret});
            })()); }
        };
    "#;
    let vm = super::super::PluginVm::from_source_sandboxed(
        "claude",
        source.into(),
        Some(manifest),
        sandbox.clone(),
    )
    .unwrap();
    let vm = Arc::new(vm);
    let provider = JsProvider::new(declaration, vm.clone());
    for account in ["a", "b", "a"] {
        with_context(Some(selected(root.path(), account, &shared)), || {
            let raw = provider.fetch_usage().unwrap().unwrap();
            let result: serde_json::Value = serde_json::from_str(&raw).unwrap();
            let native: serde_json::Value = serde_json::from_str(
                &super::super::host_credentials::public_json(sandbox.credentials(), "auth")
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(result["account"], native);
            assert_eq!(result["account"]["account"], account);
            let foreign = root.path().join(if account == "a" { "b" } else { "a" });
            assert!(!sandbox.can_read(foreign.join("models.json").to_str().unwrap()));
            assert!(!sandbox.can_read(root.path().to_str().unwrap()));
            assert_eq!(result["models"], account);
            assert_eq!(result["history"], "shared-history");
            assert!(result["secret"].is_null());
            assert!(!raw.contains("secret-"));
            let probe: serde_json::Value = super::super::call_json(&vm, "probe", &[serde_json::json!({
                "bin":std::env::current_exe().unwrap(),
                "args":["--exact","js_runtime::parity_test::account_home::account_home_exec_child_fixture","--nocapture"],
                "env":{"ACCOUNT_HOME_EXEC_CHILD":"1","CLAUDE_CONFIG_DIR":"foreign-home","ANTHROPIC_API_KEY":"foreign-credential"}
            })]).unwrap();
            assert_eq!(probe["code"], 0);
            let stdout = probe["stdout"].as_str().unwrap();
            let row = stdout
                .lines()
                .find_map(|line| line.strip_prefix("ACCOUNT_HOME_EXEC_RESULT="))
                .unwrap();
            let row: serde_json::Value = serde_json::from_str(row).unwrap();
            assert_eq!(
                PathBuf::from(row["home"].as_str().unwrap()),
                root.path().join(account)
            );
            assert_eq!(row["hasApiKey"], false);
        });
    }
    std::fs::remove_file(root.path().join("b/auth.json")).unwrap();
    with_context(Some(selected(root.path(), "b", &shared)), || {
        let result: serde_json::Value =
            serde_json::from_str(&provider.fetch_usage().unwrap().unwrap()).unwrap();
        assert!(result["account"].is_null());
    });
    assert!(crate::provider_account::current().is_none());
}

#[test]
fn account_home_exec_child_fixture() {
    if std::env::var("ACCOUNT_HOME_EXEC_CHILD").as_deref() != Ok("1") {
        return;
    }
    println!(
        "ACCOUNT_HOME_EXEC_RESULT={}",
        serde_json::json!({
            "home":std::env::var("CLAUDE_CONFIG_DIR").unwrap(),
            "hasApiKey":std::env::var_os("ANTHROPIC_API_KEY").is_some(),
        })
    );
}

#[test]
fn parity_account_home_claude_selected_session_roots_and_storage_declarations() {
    let root = tempfile::tempdir().unwrap();
    let shared = root.path().join("shared");
    let native = crate::agent_claude::provider::ClaudeProvider::new();
    let js = load_js("claude");
    assert_eq!(native.account_capability(), js.account_capability());
    assert_eq!(
        js.account_capability().unwrap().continuation,
        crate::provider_account::storage::AccountContinuation::Unverified
    );
    let unique_session = uuid::Uuid::new_v4().to_string();
    let session = unique_session.as_str();
    std::fs::create_dir_all(&shared).unwrap();
    let shared_record = shared.join(format!("{session}.jsonl"));
    std::fs::write(&shared_record, serde_json::json!({"type":"user","uuid":"shared-record","sessionId":session,"cwd":"/work","message":{"role":"user","content":"opaque-shared-history"}}).to_string()).unwrap();
    for account in ["a", "b", "a"] {
        let home = root.path().join(account);
        std::fs::create_dir_all(home.join("projects/-work")).unwrap();
        let expected = home.join("projects/-work").join(format!("{session}.jsonl"));
        if !expected.exists() {
            std::fs::hard_link(&shared_record, &expected).unwrap();
        }
        with_context(Some(selected(root.path(), account, &shared)), || {
            assert_eq!(
                PathBuf::from(native.session_file_path("/work", session).unwrap()),
                expected
            );
            assert_eq!(
                PathBuf::from(js.session_file_path("/work", session).unwrap()),
                expected
            );
            let messages = native.parse_session_messages(&expected).unwrap();
            assert_eq!(messages.len(), 1);
            assert_eq!(
                serde_json::to_value(js.parse_session_messages(&expected).unwrap()).unwrap(),
                serde_json::to_value(messages).unwrap()
            );
        });
    }
}

#[test]
fn parity_account_home_codex_uses_shared_versioned_sqlite_wal_and_index_with_private_auth() {
    let root = tempfile::tempdir().unwrap();
    let shared = root.path().join("shared");
    std::fs::create_dir_all(&shared).unwrap();
    let unique_session = uuid::Uuid::new_v4().to_string();
    let session = unique_session.as_str();
    std::fs::write(shared.join("session_index.jsonl"), format!("{}\n", serde_json::json!({"id":session,"thread_name":"Shared conversation","updated_at":"2026-11-17T00:00:00Z"}))).unwrap();
    let sqlite = shared.join("state_900.sqlite");
    let connection = rusqlite::Connection::open(&sqlite).unwrap();
    connection.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE threads (id TEXT PRIMARY KEY, model TEXT, rollout_path TEXT, cwd TEXT, title TEXT, name TEXT);").unwrap();
    connection.execute("INSERT INTO threads VALUES (?1, 'shared-model', NULL, '/work', 'Shared conversation', 'Shared conversation')", [session]).unwrap();
    assert!(shared.join("state_900.sqlite-wal").is_file());
    std::fs::write(shared.join("auth.json"), "default-secret").unwrap();
    std::fs::write(shared.join("config.toml"), "default-config").unwrap();
    std::fs::write(shared.join("state_no.sqlite"), "foreign").unwrap();
    let directory = workspace_plugins_root().join("provider-codex");
    let mut manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(directory.join("plugin.json")).unwrap())
            .unwrap();
    manifest["cliCompatibility"]["supported"] = serde_json::json!(["=1.0.0"]);
    let manifest = manifest.to_string();
    let declaration = super::super::PluginManifest::from_json_str(&manifest).unwrap();
    let sandbox = Arc::new(
        super::super::host_api::PluginSandbox::unrestricted()
            .with_identity("codex")
            .with_credentials(&declaration.credentials),
    );
    let source = std::fs::read_to_string(directory.join("plugin.js")).unwrap()
        + r#"
module.exports.default.accountFixtureListing = function(path) { return host.io.run(host.io.fs.readDir(path)); };
"#;
    let vm = super::super::provider_observation::with_fixture_version(Some("1.0.0"), || {
        super::super::PluginVm::from_source_sandboxed(
            "codex",
            source,
            Some(manifest),
            sandbox.clone(),
        )
        .unwrap()
    });
    let vm = Arc::new(vm);
    let provider = JsProvider::new(declaration, vm.clone());
    let layout = provider.account_capability().unwrap();
    assert!(layout.shared_paths.iter().any(|path| path.path == "state_"
        && path.kind == crate::provider_account::storage::StoragePathKind::VersionedSqlite
        && path.root_environment.as_deref() == Some("CODEX_SQLITE_HOME")));
    assert!(layout
        .shared_paths
        .iter()
        .any(|path| path.path == "session_index.jsonl"));
    let credential = sandbox.credentials().first().unwrap();
    for account in ["a", "b", "a"] {
        let home = root.path().join(account);
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(home.join("auth.json"), serde_json::json!({"tokens":{"account_id":account,"access_token":format!("private-{account}")}}).to_string()).unwrap();
        let context = InvocationContext::new(
            "codex".into(),
            account.into(),
            BTreeMap::from([
                (
                    "CODEX_HOME".into(),
                    Some(home.to_string_lossy().into_owned()),
                ),
                (
                    "CODEX_SQLITE_HOME".into(),
                    Some(shared.to_string_lossy().into_owned()),
                ),
                (
                    "CODETERM_CODEX_SESSION_INDEX_PATH".into(),
                    Some(
                        shared
                            .join("session_index.jsonl")
                            .to_string_lossy()
                            .into_owned(),
                    ),
                ),
                ("CODETERM_CODEX_STATE_DB_PATH".into(), None),
                ("CODETERM_CODEX_SESSIONS_ROOT".into(), None),
            ]),
            vec![home.clone()],
            vec![root.path().into()],
        )
        .unwrap()
        .with_shared_reads(
            layout
                .shared_paths
                .iter()
                .map(|path| {
                    crate::provider_account::SharedReadGrant::from_layout(path, &shared).unwrap()
                })
                .collect(),
        )
        .unwrap();
        with_context(Some(context), || {
            assert!(!sandbox.can_read(shared.join("auth.json").to_str().unwrap()));
            assert!(!sandbox.can_read(shared.join("config.toml").to_str().unwrap()));
            let rows: Vec<serde_json::Value> =
                super::super::call_json(&vm, "accountFixtureListing", &[serde_json::json!(shared)])
                    .unwrap();
            let mut actual: Vec<_> = rows
                .iter()
                .map(|row| row["name"].as_str().unwrap().to_owned())
                .collect();
            let mut expected: Vec<_> = crate::provider_account::read_directory(&shared)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
                .collect();
            actual.sort();
            expected.sort();
            assert_eq!(actual, expected);
            assert!(actual.contains(&"state_900.sqlite-wal".into()));
            for denied in ["auth.json", "config.toml", "state_no.sqlite"] {
                assert!(!actual.contains(&denied.into()));
            }
            let expected: String = connection
                .query_row(
                    "SELECT model FROM threads WHERE id = ?1",
                    [session],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                provider.detect_session_model("/work", session),
                Some(expected)
            );
            assert!(provider.find_session(session).is_some());
            assert!(provider
                .enumerate_sessions()
                .iter()
                .any(|entry| entry.session_id == session));
            let public = super::super::host_credentials::public_json(
                std::slice::from_ref(credential),
                &credential.id,
            )
            .unwrap();
            let public: serde_json::Value = serde_json::from_str(&public).unwrap();
            assert_eq!(public["accountId"], account);
            assert!(!public.to_string().contains("private-"));
            assert!(!sandbox.can_read(home.join("auth.json").to_str().unwrap()));
        });
    }
}

#[test]
fn parity_account_home_claude_session_path_uses_inherited_root_without_writes() {
    let mut env = codeterm_util::test_env::EnvGuard::lock();
    let home = tempfile::tempdir().unwrap();
    env.set("CLAUDE_CONFIG_DIR", home.path());
    let native = crate::agent_claude::provider::ClaudeProvider::new();
    let js = load_js("claude");
    let session_id = "00000000-0000-0000-0000-000000000001";
    let expected = home
        .path()
        .join("projects")
        .join("-work")
        .join(format!("{session_id}.jsonl"));
    let native_path = PathBuf::from(native.session_file_path("/work", session_id).unwrap());
    let js_path = PathBuf::from(js.session_file_path("/work", session_id).unwrap());
    assert_eq!(native_path, expected);
    assert_eq!(js_path, expected);
    assert_eq!(std::fs::read_dir(home.path()).unwrap().count(), 0);
}
