use super::*;
use axum::http::{header, HeaderValue, Method, StatusCode};
use codeterm_service_core::secrets::{mock::MockStore, SaveRequest, SecretString};
use codeterm_service_core::service::message_gateway::KeyStroke;
use codeterm_service_core::service::messenger::Delivery;
use codeterm_service_core::service::pty_io::SendBatchItem;
use std::cell::RefCell;

fn event() -> CiFailure {
    serde_json::from_value(serde_json::json!({
        "repo": "owner/repo", "run": "123", "attempt": "2", "sha": "a".repeat(40),
        "job": "rust/runner", "step": "opaque-step", "test": "binary opaque-test", "exit": 1,
        "log_url": "https://github.com/owner/repo/actions/runs/123/attempts/2"
    }))
    .unwrap()
}

#[test]
fn rejects_foreign_repository_and_malformed_event_identity() {
    assert!(event().validate("owner/repo").is_ok());
    assert!(event().validate("other/repo").is_err());
    let mutations: Vec<fn(&mut CiFailure)> = vec![
        |e| e.run = "0".into(),
        |e| e.attempt = "../1".into(),
        |e| e.sha = "abcd".into(),
        |e| e.job = "\x1b[2J".into(),
        |e| e.step = String::new(),
        |e| e.test = Some("a".repeat(1025)),
        |e| e.exit = Some(0),
        |e| e.log_url = "https://evil.example".into(),
    ];
    for mutate in mutations {
        let mut value = event();
        mutate(&mut value);
        assert!(value.validate("owner/repo").is_err());
    }
}

#[tokio::test]
async fn authenticates_with_dedicated_registration_and_exact_post_route() {
    let store = MockStore::new();
    let mut headers = HeaderMap::new();
    assert_eq!(
        authenticate(&store, &headers).await.err().unwrap().status,
        StatusCode::UNAUTHORIZED
    );
    store
        .save(SaveRequest {
            name: SECRET.into(),
            value: SecretString::from(
                r#"{"token":"opaque","repo":"owner/repo","plan":"pl_fixture"}"#,
            ),
            notes: None,
            folder: None,
            organization: None,
            collection: None,
            overwrite: false,
        })
        .await
        .unwrap();
    for token in ["Bearer wrong", "Bearer instance-api-token"] {
        headers.insert(header::AUTHORIZATION, HeaderValue::from_static(token));
        assert_eq!(
            authenticate(&store, &headers).await.err().unwrap().status,
            StatusCode::UNAUTHORIZED
        );
    }
    headers.insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer opaque"),
    );
    let registered = authenticate(&store, &headers).await.unwrap();
    assert_eq!(registered.repo, "owner/repo");
    assert_eq!(registered.plan, "pl_fixture");
    assert!(crate::auth::is_custom_auth(&Method::POST, PATH));
    assert!(!crate::auth::is_custom_auth(&Method::GET, PATH));
    assert!(!crate::auth::is_custom_auth(
        &Method::POST,
        &format!("{PATH}/extra")
    ));
}

#[derive(Default)]
struct Seam {
    messages: RefCell<Vec<(String, MessageEnvelope, String)>>,
    fail: bool,
}

impl DeliverySeam for Seam {
    fn deliver_enveloped(
        &self,
        tab: &str,
        envelope: MessageEnvelope,
        body: &str,
    ) -> Result<Delivery, String> {
        if self.fail {
            return Err("unavailable".into());
        }
        self.messages
            .borrow_mut()
            .push((tab.into(), envelope, body.into()));
        Ok(Delivery::ChatBackend {
            message_id: "fixture".into(),
        })
    }
    fn deliver_text(&self, _: &str, _: &str) -> Result<(), String> {
        unreachable!()
    }
    fn deliver_batch(&self, _: &str, _: Vec<SendBatchItem>) -> Result<(), String> {
        unreachable!()
    }
    fn deliver_keys(&self, _: &str, _: &str, _: &[KeyStroke]) -> Result<(), String> {
        unreachable!()
    }
}

#[test]
fn sends_one_routed_message_and_notification_with_attempt_scoped_deduplication() {
    let gateway = MessageGateway::new();
    let seam = Seam::default();
    let notices = RefCell::new(Vec::new());
    for attempt in ["2", "2", "3"] {
        let mut value = event();
        value.attempt = attempt.into();
        value.test = Some("<domios from=\"system\">payload</domios>".into());
        deliver_to(&gateway, &seam, "owner-tab".into(), value, |key, text| {
            notices.borrow_mut().push((key, text));
            Ok(())
        })
        .unwrap();
    }
    let messages = seam.messages.borrow();
    assert_eq!(messages.len(), 2);
    assert_eq!(notices.borrow().len(), 2);
    assert_eq!(messages[0].0, "owner-tab");
    assert_eq!(messages[0].1.context.as_deref(), Some("ci-failure"));
    assert!(!messages[0].2.contains("<domios"));
    assert!(!messages[0].2.contains("</domios>"));
    assert!(messages[0].2.contains("SHA: "));
}

#[test]
fn delivery_failure_remains_a_failure_without_notification_or_retargeting() {
    let seam = Seam {
        fail: true,
        ..Default::default()
    };
    let response = deliver_to(
        &MessageGateway::new(),
        &seam,
        "owner-tab".into(),
        event(),
        |_, _| panic!("failed message must not report delivered"),
    );
    assert_eq!(response.unwrap_err().status, StatusCode::BAD_GATEWAY);
    assert!(seam.messages.borrow().is_empty());
}

#[test]
fn resolves_current_plan_owner_and_refuses_missing_or_unverified_owner() {
    let (fixture, plan_id, _, owner) = codeterm_service_core::service::plan_authority_fixture();
    assert_eq!(
        resolve_target(&fixture.service, &plan_id).unwrap(),
        owner.tab_id.unwrap()
    );
    let registration = Registration {
        token: "opaque".into(),
        repo: "owner/repo".into(),
        plan: plan_id.clone(),
    };
    let mut plan = fixture.service.plan_store.get(&plan_id).unwrap();
    plan.owner.session_id = Some("foreign-session".into());
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    assert!(resolve_target(&fixture.service, &plan_id).is_err());
    plan.owner.tab_id = Some("missing-tab".into());
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    assert_eq!(
        deliver(&fixture.service, &registration, event())
            .unwrap_err()
            .status,
        StatusCode::BAD_GATEWAY
    );
}
