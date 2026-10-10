use super::*;
use std::collections::BTreeMap;

fn fields(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

fn codex_credential(dir: &std::path::Path) -> CredentialConfig {
    let file = dir.join("auth.json");
    std::fs::write(
        &file,
        r#"{"tokens":{"access_token":"secret-oauth-token","account_id":"acct-42"}}"#,
    )
    .unwrap();
    CredentialConfig {
        file_environment: None,
        id: "codexAuth".into(),
        file: file.to_string_lossy().into_owned(),
        public: fields(&[("accountId", "tokens.account_id")]),
        secret: fields(&[("accessToken", "tokens.access_token")]),
    }
}

#[test]
fn public_fields_resolve_and_secret_fields_never_do() {
    let dir = tempfile::tempdir().unwrap();
    let creds = vec![codex_credential(dir.path())];

    let json = public_json(&creds, "codexAuth").expect("public view resolves");
    assert!(
        json.contains("acct-42"),
        "the account id is the plugin's to read: {json}"
    );
    assert!(
        !json.contains("secret-oauth-token"),
        "a secret field must never reach the plugin: {json}"
    );
    assert_eq!(
        public_json(&creds, "undeclared"),
        None,
        "an undeclared credential resolves to nothing"
    );
}

#[test]
fn a_declared_credential_fills_the_request_header_without_exposing_the_token() {
    let dir = tempfile::tempdir().unwrap();
    let creds = vec![codex_credential(dir.path())];
    let opts = serde_json::json!({
        "url": "https://chatgpt.com/backend-api/wham/usage",
        "method": "GET",
        "headers": { "User-Agent": "codeterm-usage" },
        "credential": {
            "id": "codexAuth",
            "headers": {
                "Authorization": "Bearer {accessToken}",
                "ChatGPT-Account-Id": "{accountId}"
            }
        }
    })
    .to_string();

    let rewritten = inject(&creds, &opts).expect("injection succeeds");
    let parsed: serde_json::Value = serde_json::from_str(&rewritten).unwrap();
    assert_eq!(
        parsed["headers"]["Authorization"], "Bearer secret-oauth-token",
        "the host stamps the bearer itself"
    );
    assert_eq!(parsed["headers"]["ChatGPT-Account-Id"], "acct-42");
    assert_eq!(
        parsed["headers"]["User-Agent"], "codeterm-usage",
        "plugin-supplied headers survive"
    );
    assert!(
        parsed.get("credential").is_none(),
        "the credential request is consumed by the host, not forwarded"
    );
}

#[test]
fn supporter_token_is_added_only_when_available_and_never_replaces_plugin_authorization() {
    use codeterm_types::decision::TokenMode;

    let opts = r#"{"url":"https://example.test/path","headers":{"X-Test":"ok"}}"#;
    let stored = inject_supporter_token(TokenMode::Optional, Some("stored-secret"), opts).unwrap();
    let parsed: Value = serde_json::from_str(&stored).unwrap();
    assert_eq!(parsed["headers"]["Authorization"], "Bearer stored-secret");
    assert_eq!(
        inject_supporter_token(TokenMode::Optional, None, opts).unwrap(),
        opts
    );
    assert_eq!(
        inject_supporter_token(TokenMode::None, Some("stored-secret"), opts).unwrap(),
        opts
    );

    let explicit =
        r#"{"url":"https://example.test/path","headers":{"aUtHoRiZaTiOn":"Plugin value"}}"#;
    let with_explicit =
        inject_supporter_token(TokenMode::Required, Some("stored-secret"), explicit).unwrap();
    let parsed: Value = serde_json::from_str(&with_explicit).unwrap();
    assert_eq!(parsed["headers"]["aUtHoRiZaTiOn"], "Plugin value");
    assert_eq!(parsed["headers"].as_object().unwrap().len(), 1);
}

#[test]
fn json_pointer_credential_fields_select_dotted_keys_and_inject_the_secret() {
    let dir = tempfile::tempdir().unwrap();
    let entry = "https://auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828";
    let pointer = format!("/{}", entry.replace('~', "~0").replace('/', "~1"));
    assert_eq!(
        pointer,
        "/https:~1~1auth.x.ai::b1a00492-073a-47ea-816f-4c329264a828"
    );

    let file = dir.path().join("grok-auth.json");
    let mut document = serde_json::Map::new();
    document.insert(
        entry.into(),
        serde_json::json!({
            "key": "test-oauth-bearer",
            "user_id": "test-user-id"
        }),
    );
    std::fs::write(&file, Value::Object(document).to_string()).unwrap();

    let user_path = format!("{pointer}/user_id");
    let key_path = format!("{pointer}/key");

    let creds = vec![CredentialConfig {
        file_environment: None,
        id: "grokAuth".into(),
        file: file.to_string_lossy().into_owned(),
        public: fields(&[("userId", &user_path)]),
        secret: fields(&[("accessToken", &key_path)]),
    }];

    let public = public_json(&creds, "grokAuth").expect("public identity resolves");
    assert_eq!(
        serde_json::from_str::<Value>(&public).unwrap(),
        serde_json::json!({ "userId": "test-user-id" })
    );
    assert!(
        !public.contains("test-oauth-bearer"),
        "the OAuth bearer must not enter the plugin's public credential view"
    );

    let opts = serde_json::json!({
        "url": "https://cli-chat-proxy.grok.com/v1/billing?format=credits",
        "method": "GET",
        "credential": {
            "id": "grokAuth",
            "headers": {
                "Authorization": "Bearer {accessToken}",
                "x-userid": "{userId}"
            }
        }
    })
    .to_string();
    let rewritten = inject(&creds, &opts).expect("JSON Pointer fields inject");
    let request: Value = serde_json::from_str(&rewritten).unwrap();
    assert_eq!(
        request["headers"]["Authorization"],
        "Bearer test-oauth-bearer"
    );
    assert_eq!(request["headers"]["x-userid"], "test-user-id");
    assert!(request.get("credential").is_none());
}

#[test]
fn an_unresolvable_credential_fails_the_fetch_instead_of_sending_it_unauthenticated() {
    let dir = tempfile::tempdir().unwrap();
    let mut cred = codex_credential(dir.path());
    cred.file = dir
        .path()
        .join("missing.json")
        .to_string_lossy()
        .into_owned();
    let opts = serde_json::json!({
        "url": "https://chatgpt.com/x",
        "credential": { "id": "codexAuth", "headers": { "Authorization": "Bearer {accessToken}" } }
    })
    .to_string();

    assert!(
        inject(&[cred], &opts).is_err(),
        "a missing credential file must be an error, not a silently unauthenticated request"
    );
}

#[test]
fn opts_without_a_credential_request_pass_through_untouched() {
    let opts = r#"{"url":"https://example.com","method":"GET"}"#;
    assert_eq!(inject(&[], opts).unwrap(), opts);
}

#[test]
fn declared_files_are_absolute_paths_for_the_read_deny_list() {
    let dir = tempfile::tempdir().unwrap();
    let creds = vec![codex_credential(dir.path())];
    assert_eq!(declared_files(&creds), vec![dir.path().join("auth.json")]);
}

#[test]
fn account_home_diagnostic_credential_reads_selected_home_without_writes() {
    let mut env = codeterm_util::test_env::EnvGuard::lock();
    let default = tempfile::tempdir().unwrap();
    let selected = tempfile::tempdir().unwrap();
    let mut credential = codex_credential(default.path());
    let selected_file = selected.path().join("auth.json");
    let selected_bytes =
        r#"{"tokens":{"access_token":"selected-secret","account_id":"selected-account"}}"#;
    std::fs::write(&selected_file, selected_bytes).unwrap();
    let default_file = default.path().join("auth.json");
    let default_bytes = std::fs::read(&default_file).unwrap();
    credential.file_environment = Some(super::super::manifest::ReadRootEnvironment {
        variable: "FIXTURE_CREDENTIAL_HOME".into(),
        subdir: "auth.json".into(),
    });
    env.set("FIXTURE_CREDENTIAL_HOME", selected.path());
    let credentials = vec![credential];
    assert_eq!(
        public_json(&credentials, "codexAuth"),
        Some(r#"{"accountId":"selected-account"}"#.into())
    );
    let injected = inject(
        &credentials,
        r#"{"credential":{"id":"codexAuth","headers":{"Authorization":"Bearer {accessToken}"}}}"#,
    )
    .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&injected).unwrap()["headers"]["Authorization"],
        "Bearer selected-secret"
    );
    let denied = declared_files(&credentials);
    assert!(denied.contains(&default_file));
    assert!(denied.contains(&selected_file));
    assert_eq!(std::fs::read(&default_file).unwrap(), default_bytes);
    assert_eq!(
        std::fs::read_to_string(&selected_file).unwrap(),
        selected_bytes
    );
    env.set("FIXTURE_CREDENTIAL_HOME", "");
    assert_eq!(declared_files(&credentials), vec![default_file]);
    assert_eq!(
        public_json(&credentials, "codexAuth"),
        Some(r#"{"accountId":"acct-42"}"#.into())
    );
}
