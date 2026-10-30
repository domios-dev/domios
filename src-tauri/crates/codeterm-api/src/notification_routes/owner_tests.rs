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
        }
    }
}
