use super::*;
use crate::auth::CallerIdentity;
use axum::{body::Body, http::Request, routing::post, Router};
use codeterm_service_core::{
    multiplexer::state::MultiplexerState,
    ports::daemon_question::{DaemonQuestionHandler, DaemonQuestions},
    service::test_fixture::ServiceFixture,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tower::ServiceExt;

struct Authorization(AtomicUsize);

#[tokio::test]
async fn notify_resolve_route_closes_only_the_callers_question() {
    use codeterm_notification_core::{
        contract::NotificationAction,
        store::{Ask, NotificationProvenance, NotificationSeverity},
    };
    let (mut mux, asker) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&asker).unwrap();
    let other = mux.create_tab(&workspace).unwrap();
    let fixture = ServiceFixture::new(mux);
    let question = fixture.service.notif.lock().add_ext(
        "opaque question".into(),
        None,
        NotificationSeverity::Info,
        NotificationProvenance {
            tab_id: Some(asker.clone()),
            ..Default::default()
        },
        Ask::offering(Some(vec![NotificationAction::Answer {
            label: "opaque answer".into(),
        }])),
        None,
        None,
    );
    let state = crate::test_support::app_state(&fixture);
    let router = Router::new()
        .route(
            "/api/notifications/{id}/resolve",
            post(resolve_notification),
        )
        .with_state(state);
    for (tab, expected) in [(&other, StatusCode::CONFLICT), (&asker, StatusCode::OK)] {
        let token = fixture
            .service
            .frontend_pty_env(Some(tab))
            .remove("CODETERM_API_TOKEN")
            .unwrap();
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/notifications/{question}/resolve"))
                    .header("authorization", format!("Bearer {token}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    assert!(
        fixture
            .service
            .notif
            .lock()
            .notification(&question)
            .unwrap()
            .resolved
    );
}
impl DaemonQuestionHandler for Authorization {
    fn answer(&self, _: &str, _: &str) -> Option<Result<serde_json::Value, String>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Some(Ok(serde_json::json!({"authorized":true})))
    }
}

#[tokio::test]
async fn daemon_question_route_refuses_every_non_owner_identity_before_handler() {
    let tab = |id: &str| {
        CallerIdentity::Tab(codeterm_service_core::service::tab_credentials::TabCaller {
            tab_id: id.into(),
            binding_nonce: "bound".into(),
        })
    };
    for caller in [
        Some(CallerIdentity::Local),
        Some(tab("general")),
        Some(tab("worker")),
        Some(tab("concierge")),
        Some(tab("other")),
        Some(CallerIdentity::Peer {
            instance_id: "peer".into(),
            access_level: codeterm_service_core::mesh::types::AccessLevel::Full,
            relay_protocol_version: 1,
        }),
        Some(CallerIdentity::ServicePush {
            service_id: "push".into(),
        }),
        None,
    ] {
        let (mux, _) = MultiplexerState::new();
        let fixture = ServiceFixture::new(mux);
        let authorization = Arc::new(Authorization(AtomicUsize::new(0)));
        assert!(fixture
            .service
            .register_host_extension(Arc::new(DaemonQuestions::new(authorization.clone())))
            .is_ok());
        let id = fixture
            .service
            .publish_daemon_question(
                "fixture-question:opaque".into(),
                "Fixture".into(),
                "Opaque question".into(),
                vec!["Continue".into()],
            )
            .unwrap();
        let mut router = Router::new()
            .route("/api/notifications/{id}/answer", post(answer_notification))
            .with_state(crate::test_support::app_state(&fixture));
        let owner = matches!(&caller, Some(CallerIdentity::Local));
        if let Some(caller) = caller {
            router = router.layer(Extension(caller));
        }
        let response = router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/notifications/{id}/answer"))
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"label":"Continue"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            if owner {
                StatusCode::OK
            } else {
                StatusCode::FORBIDDEN
            }
        );
        assert_eq!(authorization.0.load(Ordering::SeqCst), usize::from(owner));
        assert_eq!(
            fixture
                .service
                .notif
                .lock()
                .answer_slot(&id)
                .unwrap()
                .delivered,
            owner
        );
        if !owner {
            assert!(fixture
                .service
                .answer_notification(&id, "Continue")
                .is_err());
            assert_eq!(authorization.0.load(Ordering::SeqCst), 0);
            let slot = fixture
                .service
                .notif
                .lock()
                .answer_slot(&id)
                .unwrap()
                .clone();
            assert!(slot.answered.is_none());
            assert!(!slot.delivered);
            fixture
                .service
                .answer_notification_as_owner(&id, "Continue")
                .unwrap();
            assert_eq!(authorization.0.load(Ordering::SeqCst), 1);
            assert!(
                fixture
                    .service
                    .notif
                    .lock()
                    .answer_slot(&id)
                    .unwrap()
                    .delivered
            );
        }
    }
}

#[tokio::test]
async fn tab_cannot_repackage_a_listed_daemon_answer_as_a_bridge_button() {
    let fixture = ServiceFixture::new(MultiplexerState::new().0);
    let handler = Arc::new(Authorization(AtomicUsize::new(0)));
    assert!(fixture
        .service
        .register_host_extension(Arc::new(DaemonQuestions::new(handler.clone())))
        .is_ok());
    let id = fixture
        .service
        .publish_daemon_question(
            "fixture:listed".into(),
            "Question".into(),
            "Opaque".into(),
            vec!["Continue".into()],
        )
        .unwrap();
    fixture
        .service
        .notif
        .lock()
        .remember_channel_placement(&id, "telegram", "42:7");
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    *fixture.service.bridge_outbound.lock() = Some(tx);
    let router = Router::new()
        .route("/api/notifications", axum::routing::get(list_notifications))
        .route(
            "/api/bridge/send",
            post(crate::config_routes::bridge_send_api),
        )
        .with_state(crate::test_support::app_state(&fixture))
        .layer(Extension(CallerIdentity::Tab(
            codeterm_service_core::service::tab_credentials::TabCaller {
                tab_id: "worker".into(),
                binding_nonce: "bound".into(),
            },
        )));
    let listed = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/notifications")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(listed.status(), StatusCode::OK);
    let records: serde_json::Value = serde_json::from_slice(
        &axum::body::to_bytes(listed.into_body(), 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    let record = records
        .as_array()
        .unwrap()
        .iter()
        .find(|record| record["id"] == id)
        .unwrap();
    let token = format!("na:{}:0", record["id"].as_str().unwrap());
    let other = fixture
        .service
        .publish_daemon_question(
            "fixture:other-listed".into(),
            "Other".into(),
            "Opaque".into(),
            vec!["Continue".into()],
        )
        .unwrap();
    fixture
        .service
        .notif
        .lock()
        .remember_channel_placement(&other, "telegram", "42:99");
    fixture
        .service
        .notif
        .lock()
        .remember_channel_placement(&id, "other-channel", "42:99");
    for key in ["data", "callback_data"] {
        let mut button = serde_json::json!({"text":"Show log"});
        button[key] = token.clone().into();
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/bridge/send")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::json!({"chat_id":42,"text":"Log","buttons":[[button]]})
                            .to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    assert!(rx.try_recv().is_err());
    for (chat, message) in [(42, Some(99)), (43, Some(7)), (42, None)] {
        assert!(fixture
            .service
            .claim_channel_answer(&token, chat, message)
            .unwrap()
            .is_err());
        assert_eq!(handler.0.load(Ordering::SeqCst), 0);
        assert!(fixture
            .service
            .notif
            .lock()
            .answer_slot(&id)
            .unwrap()
            .answered
            .is_none());
    }
    for _ in 0..2 {
        assert_eq!(
            fixture.service.claim_channel_answer(&token, 42, Some(7)),
            Some(Ok(()))
        );
    }
    assert_eq!(handler.0.load(Ordering::SeqCst), 1);
}
