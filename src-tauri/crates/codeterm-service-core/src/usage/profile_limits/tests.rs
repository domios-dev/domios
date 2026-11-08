use super::*;
use serde_json::json;

fn profile(value: serde_json::Value) -> RuntimeLaunchProfile {
    serde_json::from_value(value).unwrap()
}

#[test]
fn only_the_declared_balance_credential_in_the_plugin_namespace_resolves() {
    let required = profile(json!({"binary":"fixture-agent","args":[],
        "secretEnv":{"KEY":"domios:provider:fixture:key","OTHER":"domios:provider:fixture:other"},
        "limitsSource":{"kind":"balance","reader":"fixture-balance","credentialEnv":"KEY"}}));
    assert_eq!(
        balance_secret_name("fixture", &required, "KEY").as_deref(),
        Some("domios:provider:fixture:key")
    );
    assert_eq!(balance_secret_name("fixture", &required, "OTHER"), None);
    assert_eq!(balance_secret_name("foreign", &required, "KEY"), None);
    let optional = profile(json!({"binary":"fixture-agent","args":[],
        "authEnv":{"KEY":{"secret":"domios:provider:fixture:key","methods":["api-key"]}},
        "limitsSource":{"kind":"balance","reader":"fixture-balance","credentialEnv":"KEY"}}));
    assert_eq!(
        balance_secret_name("fixture", &optional, "KEY").as_deref(),
        Some("domios:provider:fixture:key")
    );
    let undeclared = profile(json!({"binary":"fixture-agent","args":[],
        "secretEnv":{"KEY":"domios:provider:fixture:key"},
        "limitsSource":{"kind":"unavailable","reason":"Fixture quota has no endpoint"}}));
    assert_eq!(balance_secret_name("fixture", &undeclared, "KEY"), None);
}

const SECRET_NAME: &str = "domios:provider:acp:deepseek-api-key";
const FIXTURE_KEY: &str = "sk-fixture-key";
const PROFILE: &str = "deepseek-harness";
static SHARED_FETCH_PORT: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

/// The shipped ACP bundle, registered and bound the way the server composition does it.
struct ShippedAcp {
    fixture: crate::service::test_fixture::ServiceFixture,
    _host: std::sync::Arc<dyn codeterm_plugin::PluginHostCtx>,
}

fn shipped_acp(stored_key: Option<&str>) -> ShippedAcp {
    crate::service::test_fixture::install_test_runtime();
    let fixture = crate::service::test_fixture::ServiceFixture::new(
        crate::multiplexer::state::MultiplexerState::new().0,
    );
    let store: std::sync::Arc<dyn crate::secrets::SecretStore> =
        std::sync::Arc::new(crate::secrets::mock::MockStore::new());
    crate::async_runtime::block_on(async {
        if let Some(value) = stored_key {
            store
                .save(crate::secrets::SaveRequest {
                    name: SECRET_NAME.into(),
                    value: crate::secrets::SecretString::from(value.to_string()),
                    notes: None,
                    folder: None,
                    organization: None,
                    collection: None,
                    overwrite: false,
                })
                .await
                .unwrap();
        }
        *fixture.service.secrets.write().await = store;
    });
    let manifest = crate::test_support::fixture_manifest_dir()
        .join("../../../plugins/provider-acp/plugin.json");
    let provider = crate::plugin::js_runtime::load_one(&manifest).unwrap();
    let host: std::sync::Arc<dyn codeterm_plugin::PluginHostCtx> =
        std::sync::Arc::new(fixture.service.clone());
    assert!(provider.vm().set_service(std::sync::Arc::downgrade(&host)));
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(provider));
    ShippedAcp {
        fixture,
        _host: host,
    }
}

fn answering(
    status: u16,
    body: &str,
) -> (
    codeterm_plugin::js_runtime::fetch_test_port::InstalledPort,
    std::sync::Arc<parking_lot::Mutex<Vec<(String, String, Option<String>)>>>,
) {
    use codeterm_plugin::js_runtime::fetch_test_port::{install, FetchReply};
    let seen = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
    let (recorded, body) = (seen.clone(), body.to_string());
    let port = install("api.deepseek.com", move |request| {
        recorded.lock().push((
            request.method.clone(),
            request.url.clone(),
            request.headers.get("authorization").cloned(),
        ));
        FetchReply {
            status,
            body: body.clone(),
        }
    });
    (port, seen)
}

const BALANCE_BODY: &str = r#"{"is_available":true,"balance_infos":[{"currency":"USD","total_balance":"8.42","granted_balance":"0.00","topped_up_balance":"8.42"}]}"#;

/// The live failure: the stored key ended in a CR, the header refused it before any request went out.
#[test]
fn the_shipped_plugin_reads_the_real_balance_with_a_key_stored_with_its_line_ending() {
    for stored in [
        FIXTURE_KEY.to_string(),
        format!("{FIXTURE_KEY}\r"),
        format!("{FIXTURE_KEY}\r\n"),
    ] {
        let _serial = SHARED_FETCH_PORT.lock();
        let acp = shipped_acp(Some(&stored));
        let (_port, seen) = answering(200, BALANCE_BODY);

        let snapshot = match read(&acp.fixture.service, "acp", PROFILE) {
            Ok(snapshot) => snapshot,
            Err((_, reason)) => panic!("balance read failed: {reason}"),
        };

        assert_eq!(snapshot.provider, "acp");
        assert_eq!(
            snapshot.account_id.as_deref(),
            Some("profile:deepseek-harness")
        );
        assert_eq!(
            snapshot.usage_details.unwrap()["balance"],
            serde_json::json!({"available":true,"amounts":[{"currency":"USD","amount":"8.42"}]})
        );
        assert_eq!(
            seen.lock().as_slice(),
            [(
                "GET".to_string(),
                "https://api.deepseek.com/user/balance".to_string(),
                Some(format!("Bearer {FIXTURE_KEY}")),
            )],
            "exactly one request, carrying the clean key"
        );
    }
}

fn failure_reason(acp: &ShippedAcp) -> String {
    match read(&acp.fixture.service, "acp", PROFILE) {
        Ok(_) => panic!("the read should fail"),
        Err((_, reason)) => reason,
    }
}

#[test]
fn a_failed_read_names_its_cause_and_never_the_key_or_the_response_body() {
    let _serial = SHARED_FETCH_PORT.lock();
    let (_port, seen) = answering(401, "opaque-response-body");

    let unsigned = failure_reason(&shipped_acp(None));
    assert!(unsigned.contains("credential"), "{unsigned}");
    assert!(seen.lock().is_empty(), "no key means no request");

    let refused = failure_reason(&shipped_acp(Some(FIXTURE_KEY)));
    assert!(refused.contains("401"), "{refused}");
    assert!(!refused.contains("opaque-response-body"), "{refused}");
    assert!(!refused.contains(FIXTURE_KEY), "{refused}");
}

#[test]
fn a_key_with_an_inner_control_byte_is_named_without_being_echoed() {
    let _serial = SHARED_FETCH_PORT.lock();
    let (_port, seen) = answering(200, BALANCE_BODY);

    let reason = failure_reason(&shipped_acp(Some("sk-fixture\rkey")));

    assert!(reason.contains("request failed"), "{reason}");
    assert!(!reason.contains("sk-fixture"), "{reason}");
    assert!(seen.lock().is_empty(), "a refused header sends nothing");
}

#[test]
fn only_a_balance_account_answers_with_a_failed_read_when_nothing_ever_succeeded() {
    let acp = shipped_acp(None);
    let usage = &acp.fixture.service.usage_store;
    for account in ["profile:deepseek-harness", "account-a"] {
        usage
            .insert_failure_kind(
                "acp",
                Some(account),
                None,
                UsageFailureKind::FetchError,
                "opaque cause",
                None,
            )
            .unwrap();
    }
    let latest = |account| {
        crate::service::agent_runtime::projections_latest(
            &acp.fixture.service,
            "acp",
            Some(account),
        )
        .unwrap()
    };

    let balance = latest("profile:deepseek-harness");
    assert_eq!(balance["parsed_ok"], false);
    assert!(balance["failure_reason"]
        .as_str()
        .unwrap()
        .ends_with("opaque cause"));
    assert!(latest("account-a").is_null());
}

#[test]
fn balance_readings_are_keyed_by_profile_and_never_retain_raw_payloads() {
    let raw = json!({"provider":"forged","account_id":"forged","captured_at_ms":7,"billing_mode":"api","limits_reported":true,
        "session_pct":null,"session_resets_at_ms":null,"weekly_pct":null,"weekly_resets_at_ms":null,"raw_text":"opaque-body",
        "usage_details":{"balance":{"amount":"12.50","currency":"USD","available":true}}})
    .to_string();
    let snapshot = decode("fixture", "deep-1", &raw).unwrap();
    assert_eq!(snapshot.provider, "fixture");
    assert_eq!(snapshot.account_id.as_deref(), Some("profile:deep-1"));
    assert!(snapshot.raw_text.is_empty());
    assert_eq!(
        snapshot.usage_details.unwrap()["balance"]["amount"],
        "12.50"
    );
    assert!(decode("fixture", "deep-1", "not json").is_err());
    assert!(decode("fixture", "deep-1", &"x".repeat(64 * 1024 + 1)).is_err());
}

/// Points the shipped Gemini credential declarations at a fixture home for one test.
struct GeminiHome(tempfile::TempDir);

impl GeminiHome {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir(home.path().join(".gemini")).unwrap();
        std::env::set_var("GEMINI_CLI_HOME", home.path());
        Self(home)
    }

    fn write(&self, name: &str, value: serde_json::Value) {
        std::fs::write(self.0.path().join(".gemini").join(name), value.to_string()).unwrap();
    }
}

impl Drop for GeminiHome {
    fn drop(&mut self) {
        std::env::remove_var("GEMINI_CLI_HOME");
    }
}

#[test]
fn the_shipped_plugin_reads_gemini_quota_only_for_a_google_sign_in_with_a_host_filled_token() {
    use codeterm_plugin::js_runtime::fetch_test_port::{install, FetchReply};
    const FIXTURE_TOKEN: &str = "fixture-google-token";
    let _serial = SHARED_FETCH_PORT.lock();
    let home = GeminiHome::new();
    let expiry = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
        + 3_600_000;
    home.write(
        "settings.json",
        json!({"security":{"auth":{"selectedType":"oauth-personal"}}}),
    );
    home.write(
        "oauth_creds.json",
        json!({"access_token":FIXTURE_TOKEN,"expiry_date":expiry}),
    );
    let seen = std::sync::Arc::new(parking_lot::Mutex::new(Vec::new()));
    let recorded = seen.clone();
    let _port = install("cloudcode-pa.googleapis.com", move |request| {
        recorded.lock().push((
            request.url.clone(),
            request.headers.get("authorization").cloned(),
        ));
        let body = if request.url.ends_with(":loadCodeAssist") {
            json!({"cloudaicompanionProject":"opaque-project"})
        } else {
            json!({"buckets":[{"modelId":"fixture-pro","remainingFraction":0.75}]})
        };
        FetchReply {
            status: 200,
            body: body.to_string(),
        }
    });
    let acp = shipped_acp(None);

    let snapshot = match read(&acp.fixture.service, "acp", "gemini-cli") {
        Ok(snapshot) => snapshot,
        Err((_, reason)) => panic!("quota read failed: {reason}"),
    };

    assert_eq!(snapshot.account_id.as_deref(), Some("profile:gemini-cli"));
    let details = snapshot.usage_details.unwrap();
    assert_eq!(details["quota"]["limits"][0]["label"], "fixture-pro");
    assert_eq!(details["quota"]["limits"][0]["used_percent"], 25);
    let stored = details.to_string();
    assert!(!stored.contains(FIXTURE_TOKEN), "{stored}");
    assert!(!stored.contains("opaque-project"), "{stored}");
    let bearer = Some(format!("Bearer {FIXTURE_TOKEN}"));
    assert_eq!(
        seen.lock().as_slice(),
        [
            (
                "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist".to_string(),
                bearer.clone(),
            ),
            (
                "https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota".to_string(),
                bearer,
            ),
        ],
    );

    seen.lock().clear();
    home.write(
        "settings.json",
        json!({"security":{"auth":{"selectedType":"gemini-api-key"}}}),
    );
    let reason = match read(&acp.fixture.service, "acp", "gemini-cli") {
        Ok(_) => panic!("an API-key sign-in must not read a leftover login's quota"),
        Err((_, reason)) => reason,
    };
    assert!(reason.contains("Google AI Studio"), "{reason}");
    assert!(!reason.contains(FIXTURE_TOKEN), "{reason}");
    assert!(seen.lock().is_empty(), "no quota request in API-key mode");
}
