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
