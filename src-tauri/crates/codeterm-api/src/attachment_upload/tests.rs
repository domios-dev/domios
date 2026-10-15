use super::*;
use axum::{
    body::{to_bytes, Body},
    extract::DefaultBodyLimit,
    http::Request,
    routing::post,
    Router,
};
use futures_util::{stream, StreamExt};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tower::ServiceExt;

fn router(dir: PathBuf) -> Router {
    async fn receive(
        State(dir): State<PathBuf>,
        request: axum::extract::Request,
    ) -> Result<Json<serde_json::Value>, UploadError> {
        receive_upload(
            streaming_multipart(request).await?,
            dir,
            MAX_TEMP_FILE_DECODED_BYTES,
        )
        .await
    }
    Router::new()
        .route("/api/attachments/upload", post(receive))
        .layer(DefaultBodyLimit::disable())
        .with_state(dir)
}

fn header() -> Bytes {
    Bytes::from_static(b"--attachment-boundary\r\nContent-Disposition: form-data; name=\"file\"; filename=\"note.webm\"\r\nContent-Type: audio/webm\r\n\r\n")
}

fn request(body: Body) -> Request<Body> {
    Request::post("/api/attachments/upload")
        .header(
            "content-type",
            "multipart/form-data; boundary=attachment-boundary",
        )
        .body(body)
        .unwrap()
}

#[tokio::test]
async fn multipart_round_trip_preserves_bytes_name_and_type() {
    let dir = tempfile::tempdir().unwrap();
    let data = Bytes::from_static(b"\0\x01\xfevoice-bytes");
    let body = Body::from_stream(stream::iter([
        Ok::<_, std::io::Error>(header()),
        Ok(data.clone()),
        Ok(Bytes::from_static(b"\r\n--attachment-boundary--\r\n")),
    ]));
    let response = router(dir.path().to_path_buf())
        .oneshot(request(body))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let path = PathBuf::from(result["path"].as_str().unwrap());
    assert_eq!(path.parent(), Some(dir.path()));
    assert!(path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .ends_with("-note.webm"));
    assert_eq!(result["content_type"], "audio/webm");
    assert_eq!(result["size"], data.len());
    assert_eq!(tokio::fs::read(path).await.unwrap(), data);
    assert_eq!(
        std::fs::read_dir(dir.path().join(".partial"))
            .unwrap()
            .count(),
        0
    );
}

#[tokio::test]
async fn oversized_stream_returns_typed_413_and_removes_partial_file() {
    let dir = tempfile::tempdir().unwrap();
    let chunk = Bytes::from(vec![7; 64 * 1024]);
    let count = MAX_TEMP_FILE_DECODED_BYTES / chunk.len() + 4;
    let polled = Arc::new(AtomicUsize::new(0));
    let observed = polled.clone();
    let chunks = stream::iter(
        std::iter::once(header())
            .chain(std::iter::repeat_n(chunk, count))
            .chain(std::iter::once(Bytes::from_static(
                b"\r\n--attachment-boundary--\r\n",
            ))),
    )
    .map(move |bytes| {
        observed.fetch_add(1, Ordering::SeqCst);
        Ok::<_, std::io::Error>(bytes)
    });
    let response = router(dir.path().to_path_buf())
        .oneshot(request(Body::from_stream(chunks)))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(response.headers()["content-type"], "application/json");
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(result["error"]["kind"], "attachment_too_large");
    assert_eq!(result["error"]["limit_bytes"], MAX_TEMP_FILE_DECODED_BYTES);
    assert!(
        polled.load(Ordering::SeqCst) < count + 2,
        "must reject before consuming the complete body"
    );
    assert_eq!(
        std::fs::read_dir(dir.path().join(".partial"))
            .unwrap()
            .count(),
        0
    );
    assert!(std::fs::read_dir(dir.path()).unwrap().all(|entry| entry
        .unwrap()
        .file_type()
        .unwrap()
        .is_dir()));
}

#[tokio::test]
async fn interrupted_multipart_removes_partial_file() {
    let dir = tempfile::tempdir().unwrap();
    let body = Body::from_stream(stream::iter([
        Ok::<_, std::io::Error>(header()),
        Ok(Bytes::from(vec![1; 1024])),
    ]));
    let response = router(dir.path().to_path_buf())
        .oneshot(request(body))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        std::fs::read_dir(dir.path().join(".partial"))
            .unwrap()
            .count(),
        0
    );
    assert!(std::fs::read_dir(dir.path()).unwrap().all(|entry| entry
        .unwrap()
        .file_type()
        .unwrap()
        .is_dir()));
}
