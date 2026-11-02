use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use codeterm_update::{
    application_host::{async_trait, ApplicationHost, NativeUnavailable},
    apply::{ApplyOptions, ApplyResult},
};
use std::sync::{atomic::AtomicUsize, atomic::Ordering, Arc};
use tower::ServiceExt;

struct Host {
    admits_restart: bool,
    admissions: AtomicUsize,
    restarts: AtomicUsize,
    release: parking_lot::Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    finished: parking_lot::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

#[async_trait]
impl ApplicationHost for Host {
    fn admit_restart(&self) -> Result<(), NativeUnavailable> {
        self.admissions.fetch_add(1, Ordering::SeqCst);
        self.admits_restart.then_some(()).ok_or(NativeUnavailable)
    }

    fn restart(&self) -> Result<(), String> {
        self.restarts.fetch_add(1, Ordering::SeqCst);
        self.release.lock().take().unwrap().recv().unwrap();
        let _ = self.finished.lock().take().unwrap().send(());
        Ok(())
    }

    fn exit(&self, _: i32) -> Result<(), String> {
        panic!("unexpected exit")
    }

    async fn copy_image_to_clipboard(&self, _: String) -> Result<(), String> {
        panic!("unexpected clipboard access")
    }

    async fn check_and_apply(&self, _: ApplyOptions) -> Result<ApplyResult, String> {
        panic!("unexpected update")
    }
}

struct ReleaseOnDrop(Option<std::sync::mpsc::Sender<()>>);

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

#[tokio::test]
async fn restart_admission_api_capability_matrix() {
    for capability in [None, Some(false), Some(true)] {
        let (mux, _) = MultiplexerState::new();
        let fixture = ServiceFixture::new(mux);
        let (release, released) = std::sync::mpsc::channel();
        let release = ReleaseOnDrop(Some(release));
        let (finished, completion) = tokio::sync::oneshot::channel();
        let host = Arc::new(Host {
            admits_restart: capability == Some(true),
            admissions: AtomicUsize::new(0),
            restarts: AtomicUsize::new(0),
            release: parking_lot::Mutex::new(Some(released)),
            finished: parking_lot::Mutex::new(Some(finished)),
        });
        if capability.is_some() {
            assert!(fixture
                .service
                .register_host_extension(Arc::new(
                    codeterm_commands::application_host::ApplicationHostState(host.clone())
                ))
                .is_ok());
        }
        let state = crate::transport_tabs::fixture::app_state(&fixture);
        let app = crate::routes::create_router(state);
        let response = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            app.oneshot(
                Request::post("/api/app/restart")
                    .header("authorization", "Bearer preview-test-token")
                    .body(Body::empty())
                    .unwrap(),
            ),
        )
        .await
        .expect("restart response must not wait for restart execution")
        .unwrap();
        assert_eq!(response.headers()["content-type"], "application/json");
        let expected = if capability == Some(true) {
            StatusCode::OK
        } else {
            StatusCode::CONFLICT
        };
        assert_eq!(response.status(), expected, "{capability:?}");
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        assert_eq!(
            host.admissions.load(Ordering::SeqCst),
            usize::from(capability.is_some())
        );
        if capability == Some(true) {
            assert_eq!(body, serde_json::json!({"ok": true}));
            drop(release);
            tokio::time::timeout(std::time::Duration::from_millis(500), completion)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(host.restarts.load(Ordering::SeqCst), 1);
        } else {
            assert_eq!(body["code"], "native_capability_unavailable");
            assert!(body.get("ok").is_none());
            assert!(body["message"].as_str().unwrap().contains("local launcher"));
            assert_eq!(host.restarts.load(Ordering::SeqCst), 0);
        }
    }
}
