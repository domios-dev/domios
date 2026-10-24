use super::*;
use tokio::sync::Mutex;
fn installation() -> InstallationId {
    InstallationId::parse("fixture-installation".into()).unwrap()
}
fn flow() -> LoginFlow {
    LoginFlow::from_parts(
        "fixture-state-matching".into(),
        Secret::new("fixture-verifier"),
        "domios://auth/callback".into(),
    )
}
fn callback(state: &str) -> Callback {
    Callback {
        code: "c".repeat(43),
        state: state.into(),
    }
}
struct Transport(Mutex<Vec<(String, Body, Option<RequestDiagnostic>)>>);
#[async_trait]
impl super::Transport for Transport {
    async fn post(
        &self,
        url: &str,
        body: Body,
        diagnostic: Option<RequestDiagnostic>,
    ) -> Result<serde_json::Value> {
        let mut requests = self.0.lock().await;
        let response = match requests.len() {
            0 => serde_json::json!({"custom_token":"custom-fixture"}),
            1 => {
                serde_json::json!({"idToken":"id-fixture", "refreshToken":"refresh-fixture", "expiresIn":"3600"})
            }
            2 => {
                serde_json::json!({"users":[{"localId":"uid-fixture","email":"fixture@example.test","displayName":"Fixture"}]})
            }
            _ => return Err(Error::Transport),
        };
        requests.push((url.into(), body, diagnostic));
        Ok(response)
    }
}
#[tokio::test]
async fn redeem_and_token_exchange_produce_session() {
    let transport = Arc::new(Transport(Mutex::new(Vec::new())));
    let provider = Firebase::new(fixture_config(), transport.clone(), installation());
    let tokens = provider
        .redeem(flow().complete(callback("fixture-state-matching")).unwrap())
        .await
        .unwrap();
    assert_eq!(tokens.session.identity.uid, "uid-fixture");
    assert_eq!(
        tokens.session.identity.email.as_deref(),
        Some("fixture@example.test")
    );
    assert_eq!(tokens.session.id_token.expose(), "id-fixture");
    assert!(tokens.session.expires_at.is_some());
    let requests = transport.0.lock().await;
    assert_eq!(requests[0].2, Some(RequestDiagnostic::BackendRedeem));
    assert_eq!(
        requests[1].2,
        Some(RequestDiagnostic::FirebaseCustomTokenExchange)
    );
    let Body::Json(body) = &requests[0].1 else {
        panic!()
    };
    assert_eq!(body["installation_id"], "fixture-installation");
    assert_eq!(body["code_verifier"], "fixture-verifier");
    assert_eq!(body["state"], "fixture-state-matching");
    assert_eq!(body["redirect_uri"], "domios://auth/callback");
    let Body::Json(body) = &requests[1].1 else {
        panic!()
    };
    assert_eq!(body["token"], "custom-fixture");
    assert!(requests.iter().all(|(url, _, _)| !url.contains("fixture")));
}

struct RefreshTransport;
#[async_trait]
impl super::Transport for RefreshTransport {
    async fn post(
        &self,
        url: &str,
        body: Body,
        diagnostic: Option<RequestDiagnostic>,
    ) -> Result<Value> {
        assert_eq!(diagnostic, None);
        if url.starts_with("https://securetoken.googleapis.com/") {
            let Body::Form(form) = body else { panic!() };
            assert_eq!(
                form,
                vec![
                    ("grant_type".into(), "refresh_token".into()),
                    ("refresh_token".into(), "refresh-before".into())
                ]
            );
            Ok(json!({"id_token":"id-after", "refresh_token":"refresh-after", "expires_in":"3600"}))
        } else {
            let Body::Json(body) = body else { panic!() };
            assert_eq!(body["idToken"], "id-after");
            Ok(json!({"users":[{"localId":"uid-after"}]}))
        }
    }
}
#[tokio::test]
async fn refresh_posts_form_and_replaces_tokens() {
    let provider = Firebase::new(fixture_config(), Arc::new(RefreshTransport), installation());
    let tokens = provider
        .refresh(&Secret::new("refresh-before"))
        .await
        .unwrap();
    assert_eq!(tokens.session.id_token.expose(), "id-after");
    assert!(tokens.session.expires_at.is_some());
    assert_eq!(tokens.refresh_token.expose(), "refresh-after");
    assert_eq!(tokens.session.identity.uid, "uid-after");
}
struct LookupTransport(Value);
#[async_trait]
impl super::Transport for LookupTransport {
    async fn post(&self, url: &str, _: Body, _: Option<RequestDiagnostic>) -> Result<Value> {
        if url.starts_with("https://securetoken.googleapis.com/") {
            Ok(json!({"id_token":"id-after", "refresh_token":"refresh-after", "expires_in":"3600"}))
        } else {
            Ok(json!({ "users": [self.0.clone()] }))
        }
    }
}
async fn refreshed_photo(user: Value) -> Option<String> {
    let provider = Firebase::new(
        fixture_config(),
        Arc::new(LookupTransport(user)),
        installation(),
    );
    let tokens = provider
        .refresh(&Secret::new("refresh-before"))
        .await
        .unwrap();
    tokens.session.identity.photo_url
}
#[tokio::test]
async fn lookup_accepts_only_bounded_https_photo_urls() {
    let photo = "https://lh3.googleusercontent.com/a/fixture=s96-c";
    assert_eq!(
        refreshed_photo(json!({"localId":"uid", "photoUrl": photo}))
            .await
            .as_deref(),
        Some(photo)
    );
    let oversize = format!("https://lh3.googleusercontent.com/{}", "a".repeat(2048));
    for rejected in [
        "http://lh3.googleusercontent.com/a/fixture",
        "javascript:alert(1)",
        "data:image/png;base64,AAAA",
        "https://user:pass@lh3.googleusercontent.com/a",
        "not a url",
        "",
        oversize.as_str(),
    ] {
        assert_eq!(
            refreshed_photo(json!({"localId":"uid", "photoUrl": rejected})).await,
            None
        );
    }
    assert_eq!(refreshed_photo(json!({"localId":"uid"})).await, None);
}
#[tokio::test]
async fn lookup_falls_back_to_linked_provider_photo() {
    let photo = "https://platform-lookaside.fbsbx.com/platform/profilepic/?asid=1";
    let user = json!({"localId":"uid", "photoUrl":"http://insecure.test/a", "providerUserInfo":[
        {"providerId":"password"}, {"providerId":"facebook.com", "photoUrl": photo}
    ]});
    assert_eq!(refreshed_photo(user).await.as_deref(), Some(photo));
}
#[test]
fn identity_debug_redacts_photo_url() {
    let identity = Identity {
        uid: "uid".into(),
        email: None,
        display_name: None,
        provider: "firebase".into(),
        photo_url: Some("https://lh3.googleusercontent.com/a/secret-path".into()),
    };
    let debug = format!("{identity:?}");
    assert!(!debug.contains("secret-path"));
    assert!(debug.contains("[REDACTED]"));
}
#[test]
fn identity_without_photo_url_deserializes() {
    let identity: Identity =
        serde_json::from_value(json!({"uid":"u","email":null,"display_name":null,"provider":"p"}))
            .unwrap();
    assert_eq!(identity.photo_url, None);
}
struct RefusingTransport;
#[async_trait]
impl super::Transport for RefusingTransport {
    async fn post(&self, _: &str, _: Body, _: Option<RequestDiagnostic>) -> Result<Value> {
        Err(Error::Provider)
    }
}
#[tokio::test]
async fn rejected_redemption_cannot_create_session() {
    let provider = Firebase::new(
        fixture_config(),
        Arc::new(RefusingTransport),
        installation(),
    );
    assert!(matches!(
        provider
            .redeem(flow().complete(callback("fixture-state-matching")).unwrap())
            .await,
        Err(Error::Provider)
    ));
}

#[test]
fn backend_error_codes_are_logged_only_when_they_match_the_safe_pattern() {
    assert_eq!(
        safe_backend_error_code(Some(&json!({"error":{"message":"INVALID_CUSTOM_TOKEN"}}))),
        "INVALID_CUSTOM_TOKEN"
    );
    assert_eq!(
        safe_backend_error_code(Some(&json!({"error":"Unable to complete desktop sign-in"}))),
        "unrecognized"
    );
    assert_eq!(
        safe_backend_error_code(Some(&json!({"error":{"message":"bad-code"}}))),
        "unrecognized"
    );
    assert_eq!(
        safe_backend_error_code(Some(&json!({"error":{"message":"A".repeat(65)}}))),
        "unrecognized"
    );
    assert_eq!(
        safe_backend_error_code(Some(&json!({"code":"A".repeat(43)}))),
        "unrecognized"
    );
    assert_eq!(safe_backend_error_code(None), "unrecognized");
}

#[test]
fn refresh_http_failures_sign_out_only_for_credential_rejections() {
    for status in [429, 500, 503] {
        assert_eq!(classify_http_error(status, None), Error::Transport);
        assert_eq!(
            classify_http_error(
                status,
                Some(&json!({"error":{"message":"INVALID_REFRESH_TOKEN"}}))
            ),
            Error::Transport
        );
    }
    assert_eq!(
        classify_http_error(
            400,
            Some(&json!({"error":{"message":"INVALID_REFRESH_TOKEN"}}))
        ),
        Error::Credentials
    );
    assert_eq!(
        classify_http_error(400, Some(&json!({"error":{"message":"USER_DISABLED"}}))),
        Error::Credentials
    );
    assert_eq!(
        classify_http_error(400, Some(&json!({"error":{"message":"SOME_OTHER_ERROR"}}))),
        Error::Transport
    );
}

fn fixture_config() -> Config {
    Config {
        provider: "firebase".into(),
        api_key: "test-key".into(),
        redeem_url: "https://example.test/redeem".into(),
        signin_url: "https://example.test/signin".into(),
        ..Config::default()
    }
}
#[test]
fn build_config_availability_and_overrides() {
    use crate::build_config::Vendor;
    let missing = config_from_build(Vendor::default());
    assert!(!available(&missing));
    assert_eq!(
        crate::adapters::configured(
            &Config {
                provider: "firebase".into(),
                ..missing
            },
            installation()
        )
        .unwrap()
        .id(),
        "local"
    );
    let configured = config_from_build(Vendor {
        api_key: Some("test-key"),
        project_id: Some("test-project"),
        app_id: Some("test-app"),
        redeem_url: Some("https://example.test/redeem"),
        ..Vendor::default()
    });
    assert!(available(&configured));
    assert_eq!(configured.provider, "firebase");
    assert_eq!(configured.api_key, "test-key");
    assert_eq!(configured.signin_url, "https://auth.domios.dev/signin/");
    assert_eq!(configured.redeem_url, "https://example.test/redeem");
    assert_eq!(
        crate::adapters::configured(&fixture_config(), installation())
            .unwrap()
            .id(),
        "firebase"
    );
}

#[test]
fn hosted_signin_url_preserves_protocol_with_default_and_override() {
    for (signin_url, host) in [
        (None, "auth.domios.dev"),
        (Some("  "), "auth.domios.dev"),
        (Some("https://qa.example.test/signin/"), "qa.example.test"),
    ] {
        let config = config_from_build(crate::build_config::Vendor {
            signin_url,
            ..crate::build_config::Vendor::default()
        });
        for (mode, expected_mode) in [(LoginMode::Signin, "signin"), (LoginMode::Signup, "signup")]
        {
            let flow = flow().with_hint(LoginHint {
                provider: Some(LoginProvider::Email),
                mode: Some(mode),
            });
            let url = flow.signin_url(&config.signin_url).unwrap();
            assert_eq!(url.scheme(), "https");
            assert_eq!(url.host_str(), Some(host));
            assert_eq!(url.path(), "/signin/");
            assert_eq!(
                url.query_pairs().into_owned().collect::<Vec<_>>(),
                vec![
                    ("state".into(), "fixture-state-matching".into()),
                    (
                        "code_challenge".into(),
                        LoginFlow::challenge("fixture-verifier"),
                    ),
                    ("redirect_uri".into(), "domios://auth/callback".into()),
                    ("provider".into(), "email".into()),
                    ("mode".into(), expected_mode.into()),
                ]
            );
        }
    }
}
