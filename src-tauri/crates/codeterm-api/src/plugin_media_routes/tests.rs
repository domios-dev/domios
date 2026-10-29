use super::*;
use axum::{
    http::Request,
    routing::{delete, get, post},
    Router,
};
use codeterm_service_core::{
    multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture,
};
use tower::ServiceExt;

#[test]
fn single_byte_ranges_cover_seeking_and_reject_overflow_and_multiple_ranges() {
    assert_eq!(
        byte_range(None, 10),
        Ok(ByteRange {
            start: 0,
            end: 9,
            partial: false
        })
    );
    for (value, start, end) in [
        ("bytes=2-4", 2, 4),
        ("bytes=2-", 2, 9),
        ("bytes=-3", 7, 9),
        ("bytes=0-999", 0, 9),
        ("bytes=-99", 0, 9),
    ] {
        assert_eq!(
            byte_range(Some(value), 10),
            Ok(ByteRange {
                start,
                end,
                partial: true
            }),
            "{value}"
        );
    }
    for value in [
        "bytes=10-",
        "bytes=9-8",
        "bytes=-0",
        "bytes=0-1,4-5",
        "bytes=+1-2",
        "bytes=0-18446744073709551616",
        "items=0-1",
        "bytes=1--2",
        "bytes=-",
        "bytes= 0-1",
    ] {
        assert!(byte_range(Some(value), 10).is_err(), "{value}");
    }
    assert!(byte_range(Some(&"x".repeat(129)), 10).is_err());
    assert!(byte_range(None, 0).is_err());
}
#[test]
fn bearer_exemption_is_only_exact_get_head_capability_paths() {
    let token = "a".repeat(64);
    let path = format!("/api/plugins/example/media/{token}/{token}");
    for method in [Method::GET, Method::HEAD] {
        assert!(crate::auth::is_custom_auth(&method, &path));
    }
    for method in [Method::POST, Method::DELETE, Method::PUT] {
        assert!(!crate::auth::is_custom_auth(&method, &path));
    }
    for path in [
        format!("{path}/extra"),
        path.replace("example", ".."),
        path.replace(&token, "short"),
        format!("/api/plugins/example/media-register/{token}"),
        format!("/api/plugins/example/media-frame/{token}"),
    ] {
        for method in [Method::GET, Method::HEAD, Method::POST, Method::DELETE] {
            assert!(
                !crate::auth::is_custom_auth(&method, &path),
                "{method} {path}"
            );
        }
    }
}
#[tokio::test]
async fn actual_routes_refuse_unknown_handles_and_non_owner_registration_without_metadata() {
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = crate::test_support::app_state(&fixture);
    let router = Router::new()
        .route("/api/plugins/{id}/media/{frame}/{handle}", get(playback))
        .route("/api/plugins/{id}/media-register/{frame}", post(register))
        .route("/api/plugins/{id}/media-frame/{frame}", delete(revoke))
        .with_state(state);
    let token = "a".repeat(64);
    for method in ["GET", "HEAD"] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(format!("/api/plugins/example/media/{token}/{token}"))
                    .header("range", "bytes=999999-")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(!response.headers().contains_key(header::CONTENT_RANGE));
        assert_ne!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("video/mp4")
        );
    }
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/plugins/example/media-register/{token}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"id":"own","viewFrame":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = router
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/plugins/example/media-frame/{token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[derive(Clone)]
struct Reader {
    bytes: std::sync::Arc<Vec<u8>>,
    reads: std::sync::Arc<parking_lot::Mutex<Vec<(u64, usize)>>>,
}
impl MediaReader for Reader {
    fn size(&self) -> u64 {
        self.bytes.len() as u64
    }
    fn content_type(&self) -> String {
        "video/mp4".into()
    }
    fn read(&self, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
        self.reads.lock().push((offset, length));
        Ok(self.bytes[offset as usize..offset as usize + length].to_vec())
    }
}
#[tokio::test]
async fn range_get_head_and_full_stream_preserve_headers_and_bounded_reads() {
    let source = Reader {
        bytes: std::sync::Arc::new((0..150000).map(|i| (i % 251) as u8).collect()),
        reads: Default::default(),
    };
    for (range, start, end) in [
        (None, 0, 149999),
        (Some("bytes=7-21"), 7, 21),
        (Some("bytes=-20"), 149980, 149999),
        (Some("bytes=149990-"), 149990, 149999),
    ] {
        let mut headers = HeaderMap::new();
        if let Some(range) = range {
            headers.insert(header::RANGE, HeaderValue::from_static(range));
        }
        source.reads.lock().clear();
        let response = playback_response(source.clone(), Method::GET, headers.clone()).unwrap();
        assert_eq!(
            response.status(),
            if range.is_some() {
                StatusCode::PARTIAL_CONTENT
            } else {
                StatusCode::OK
            }
        );
        assert_eq!(response.headers()[header::CONTENT_TYPE], "video/mp4");
        assert_eq!(
            response.headers()[header::CONTENT_LENGTH],
            (end - start + 1).to_string()
        );
        assert_eq!(response.headers()[header::ACCEPT_RANGES], "bytes");
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(
            response.headers()[header::X_CONTENT_TYPE_OPTIONS],
            "nosniff"
        );
        if range.is_some() {
            assert_eq!(
                response.headers()[header::CONTENT_RANGE],
                format!("bytes {start}-{end}/150000")
            );
        }
        let body = axum::body::to_bytes(response.into_body(), 200000)
            .await
            .unwrap();
        assert_eq!(&body[..], &source.bytes[start..=end]);
        assert!(source
            .reads
            .lock()
            .iter()
            .all(|(_, length)| *length <= 65536));
        source.reads.lock().clear();
        let head = playback_response(source.clone(), Method::HEAD, headers).unwrap();
        assert_eq!(
            head.headers()[header::CONTENT_LENGTH],
            (end - start + 1).to_string()
        );
        assert!(axum::body::to_bytes(head.into_body(), 1)
            .await
            .unwrap()
            .is_empty());
        assert!(source.reads.lock().is_empty());
    }
    let mut headers = HeaderMap::new();
    headers.insert(header::RANGE, HeaderValue::from_static("bytes=999999-"));
    let response = playback_response(source.clone(), Method::GET, headers).unwrap();
    assert_eq!(response.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(response.headers()[header::CONTENT_RANGE], "bytes */150000");
}

struct DeferredMedia {
    id: String,
    entered: parking_lot::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    release: std::sync::Arc<tokio::sync::Notify>,
    contexts: std::sync::Arc<parking_lot::Mutex<Vec<codeterm_plugin::view_host::ViewCallContext>>>,
}
#[async_trait::async_trait]
impl codeterm_plugin::view_host::ViewProvider for DeferredMedia {
    fn id(&self) -> &str {
        &self.id
    }
    fn media_delivery(&self) -> Option<media::MediaDeliveryDescriptor> {
        Some(media::MediaDeliveryDescriptor {
            types: vec![media::MediaType::Mp4],
        })
    }
    async fn media_file(
        &self,
        _id: &str,
        context: &codeterm_plugin::view_host::ViewCallContext,
    ) -> Result<media::MediaFile, codeterm_plugin::view_host::ViewError> {
        self.contexts.lock().push(context.clone());
        if let Some(entered) = self.entered.lock().take() {
            let _ = entered.send(());
        }
        self.release.notified().await;
        Ok(media::MediaFile {
            path: "generated/clip.mp4".into(),
            file_name: None,
            content_type: media::MediaType::Mp4,
        })
    }
    async fn call(
        &self,
        _method: &str,
        _args: serde_json::Value,
        _context: &codeterm_plugin::view_host::ViewCallContext,
    ) -> Result<serde_json::Value, codeterm_plugin::view_host::ViewError> {
        Ok(serde_json::Value::Null)
    }
}
#[tokio::test]
async fn media_lookup_receives_registered_context_and_never_publishes_after_frame_or_provider_loss()
{
    use codeterm_plugin::view_host::{ViewProvider, ViewRegistryHandle};
    use std::sync::Arc;
    for provider_lost in [false, true] {
        let fixture = ServiceFixture::new(MultiplexerState::empty());
        let plugin = format!("media-context-{}", uuid::Uuid::new_v4().simple());
        let (entered, ready) = tokio::sync::oneshot::channel();
        let release = Arc::new(tokio::sync::Notify::new());
        let contexts = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let provider: Arc<dyn ViewProvider> = Arc::new(DeferredMedia {
            id: plugin.clone(),
            entered: parking_lot::Mutex::new(Some(entered)),
            release: release.clone(),
            contexts: contexts.clone(),
        });
        let registry = fixture.service.ext::<ViewRegistryHandle>();
        registry.register(&plugin, provider.clone());
        let context = fixture
            .service
            .plugin_view_context(&plugin, None, None, None)
            .unwrap();
        let registration = media::create_frame(&provider, registry, context).unwrap();
        let expected = media::frame_context(&plugin, &registration.media_frame).unwrap();
        let state = crate::test_support::app_state(&fixture);
        let router = Router::new()
            .route("/api/plugins/{id}/media-register/{frame}", post(register))
            .layer(Extension(CallerIdentity::Local))
            .with_state(state);
        let request = Request::builder()
            .method("POST")
            .uri(format!(
                "/api/plugins/{plugin}/media-register/{}",
                registration.media_frame
            ))
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({"id":"output","viewFrame":registration.view_lease}).to_string(),
            ))
            .unwrap();
        let response = tokio::spawn(async move { router.oneshot(request).await.unwrap() });
        tokio::time::timeout(std::time::Duration::from_secs(2), ready)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(contexts.lock().as_slice(), &[expected]);
        if provider_lost {
            registry.unregister(&plugin);
        } else {
            media::revoke_frame(&plugin, &registration.media_frame);
        }
        release.notify_one();
        let response = tokio::time::timeout(std::time::Duration::from_secs(2), response)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        assert!(!String::from_utf8_lossy(&body).contains("handle"));
        media::revoke_plugin(&plugin);
    }
}

#[tokio::test]
async fn late_old_media_request_cannot_adopt_or_revoke_a_reminted_document() {
    use codeterm_plugin::view_host::{ViewProvider, ViewRegistryHandle};
    use std::sync::Arc;
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let plugin = format!("media-old-lease-{}", uuid::Uuid::new_v4().simple());
    let (entered, _ready) = tokio::sync::oneshot::channel();
    let contexts = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let provider: Arc<dyn ViewProvider> = Arc::new(DeferredMedia {
        id: plugin.clone(),
        entered: parking_lot::Mutex::new(Some(entered)),
        release: Arc::new(tokio::sync::Notify::new()),
        contexts: contexts.clone(),
    });
    let registry = fixture.service.ext::<ViewRegistryHandle>();
    registry.register(&plugin, provider.clone());
    let context = fixture
        .service
        .plugin_view_context(&plugin, None, None, None)
        .unwrap();
    let old = media::create_frame(&provider, registry, context.clone()).unwrap();
    media::revoke_frame(&plugin, &old.media_frame);
    let fresh = media::reregister_frame(&provider, registry, context, &old.media_frame).unwrap();
    let router = Router::new()
        .route("/api/plugins/{id}/media-register/{frame}", post(register))
        .layer(Extension(CallerIdentity::Local))
        .with_state(crate::test_support::app_state(&fixture));
    let request = Request::builder()
        .method("POST")
        .uri(format!(
            "/api/plugins/{plugin}/media-register/{}",
            old.media_frame
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::json!({"id":"output","viewFrame":old.view_lease}).to_string(),
        ))
        .unwrap();
    let response = tokio::time::timeout(std::time::Duration::from_secs(2), router.oneshot(request))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(contexts.lock().is_empty());
    media::view_frame(&plugin, &fresh.view_lease).unwrap();
    media::revoke_plugin(&plugin);
}

#[derive(Clone)]
struct ImageReader(Reader);
impl MediaReader for ImageReader{
    fn size(&self)->u64{self.0.size()}
    fn content_type(&self)->String{"image/png".into()}
    fn read(&self,offset:u64,length:usize)->std::io::Result<Vec<u8>>{self.0.read(offset,length)}
}
#[tokio::test]
async fn image_delivery_uses_passive_mime_no_store_and_nosniff_with_bounded_reads(){
    let source=ImageReader(Reader{bytes:std::sync::Arc::new(vec![137,80,78,71,13,10,26,10]),reads:Default::default()});
    for method in [Method::GET,Method::HEAD]{
        let response=playback_response(source.clone(),method.clone(),HeaderMap::new()).unwrap();assert_eq!(response.status(),StatusCode::OK);assert_eq!(response.headers()[header::CONTENT_TYPE],"image/png");assert_eq!(response.headers()[header::CACHE_CONTROL],"no-store");assert_eq!(response.headers()[header::X_CONTENT_TYPE_OPTIONS],"nosniff");assert_eq!(response.headers()[header::CONTENT_LENGTH],"8");
        let bytes=axum::body::to_bytes(response.into_body(),8).await.unwrap();if method==Method::GET{assert_eq!(bytes.as_ref(),source.0.bytes.as_slice());}else{assert!(bytes.is_empty());}
    }
}
