use super::*;

#[tokio::test]
async fn fixture_import_streams_past_the_json_body_bound_without_retaining_history() {
    let document = super::super::tests::fixture_document();
    let mut header = serde_json::to_vec(&document).unwrap();
    header.pop();
    header.extend_from_slice(b",\"history\":\"");
    let chunks = futures_util::stream::iter(
        std::iter::once(Ok::<_, std::io::Error>(axum::body::Bytes::from(header)))
            .chain((0..33 * 16).map(|_| Ok(axum::body::Bytes::from(vec![b'x'; 64 * 1024]))))
            .chain(std::iter::once(Ok(axum::body::Bytes::from_static(b"\"}")))),
    );
    let (snapshot, plans) = receive(Body::from_stream(chunks)).await.unwrap();
    assert_eq!(snapshot.workspaces[0].tabs[0].id, "tab");
    assert!(plans.is_empty());
}

#[tokio::test]
async fn interrupted_fixture_stream_refuses_before_import() {
    let chunks = futures_util::stream::iter([
        Ok(axum::body::Bytes::from_static(b"{\"format\":")),
        Err(std::io::Error::other("fixture interrupted")),
    ]);
    assert!(receive(Body::from_stream(chunks)).await.is_err());
}
