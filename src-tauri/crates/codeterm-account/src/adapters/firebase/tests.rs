use super::*;
use tokio::sync::Mutex;
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
struct Transport(Mutex<Vec<(String, Body)>>);
#[async_trait]
impl super::Transport for Transport {
    async fn post(&self, url: &str, body: Body) -> Result<serde_json::Value> {
        let mut requests = self.0.lock().await;
        let response = match requests.len() {
            0 => serde_json::json!({"custom_token":"custom-fixture"}),
            1 => serde_json::json!({"idToken":"id-fixture", "refreshToken":"refresh-fixture"}),
            2 => {
                serde_json::json!({"users":[{"localId":"uid-fixture","email":"fixture@example.test","displayName":"Fixture"}]})
            }
            _ => return Err(Error::Transport),
        };
        requests.push((url.into(), body));
        Ok(response)
    }
}
#[tokio::test]
async fn redeem_and_token_exchange_produce_session() {
    let transport = Arc::new(Transport(Mutex::new(Vec::new())));
    let provider = Firebase::new(Config::default(), transport.clone());
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
    let requests = transport.0.lock().await;
    let Body::Json(body) = &requests[0].1 else {
        panic!()
    };
    assert_eq!(body["code_verifier"], "fixture-verifier");
    assert_eq!(body["state"], "fixture-state-matching");
    assert_eq!(body["redirect_uri"], "domios://auth/callback");
    let Body::Json(body) = &requests[1].1 else {
        panic!()
    };
    assert_eq!(body["token"], "custom-fixture");
    assert!(requests.iter().all(|(url, _)| !url.contains("fixture")));
}

struct RefreshTransport;
#[async_trait]
impl super::Transport for RefreshTransport {
    async fn post(&self, url: &str, body: Body) -> Result<Value> {
        if url.starts_with("https://securetoken.googleapis.com/") {
            let Body::Form(form) = body else { panic!() };
            assert_eq!(
                form,
                vec![
                    ("grant_type".into(), "refresh_token".into()),
                    ("refresh_token".into(), "refresh-before".into())
                ]
            );
            Ok(json!({"id_token":"id-after", "refresh_token":"refresh-after"}))
        } else {
            let Body::Json(body) = body else { panic!() };
            assert_eq!(body["idToken"], "id-after");
            Ok(json!({"users":[{"localId":"uid-after"}]}))
        }
    }
}
#[tokio::test]
async fn refresh_posts_form_and_replaces_tokens() {
    let provider = Firebase::new(Config::default(), Arc::new(RefreshTransport));
    let tokens = provider
        .refresh(&Secret::new("refresh-before"))
        .await
        .unwrap();
    assert_eq!(tokens.session.id_token.expose(), "id-after");
    assert_eq!(tokens.refresh_token.expose(), "refresh-after");
    assert_eq!(tokens.session.identity.uid, "uid-after");
}
struct RefusingTransport;
#[async_trait]
impl super::Transport for RefusingTransport {
    async fn post(&self, _: &str, _: Body) -> Result<Value> {
        Err(Error::Provider)
    }
}
#[tokio::test]
async fn rejected_redemption_cannot_create_session() {
    let provider = Firebase::new(Config::default(), Arc::new(RefusingTransport));
    assert!(matches!(
        provider
            .redeem(flow().complete(callback("fixture-state-matching")).unwrap())
            .await,
        Err(Error::Provider)
    ));
}
