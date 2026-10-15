use crate::stream_census::{track, StreamGuard};
use axum::body::{Body, Bytes};
use futures_core::Stream;
use futures_util::StreamExt;

use crate::output_stream;
use crate::stream_diagnostics::{StreamDiagnostics, StreamTabStart};

pub(crate) fn output_body<S>(
    upstream: S,
    level: Option<log::Level>,
    tab_scope: String,
    stream_tab_id: Option<String>,
    since: Option<&str>,
    mut guard: StreamGuard,
) -> Body
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
{
    let Some(level) = level else {
        return Body::from_stream(track(upstream, guard));
    };

    let cursor = since.and_then(output_stream::parse_cursor);
    let cursor_boot = cursor.as_ref().map(|value| value.boot.clone());
    let cursor_offset = stream_tab_id
        .as_deref()
        .and_then(|tab| cursor.as_ref()?.tabs.get(tab))
        .copied();
    let mut diagnostics = StreamDiagnostics::new(
        level,
        "relay",
        uuid::Uuid::new_v4().simple().to_string(),
        tab_scope,
        cursor_boot.clone(),
        cursor_offset,
    );
    if let Some(tab_id) = stream_tab_id {
        diagnostics.attach_tab(StreamTabStart {
            tab_id,
            pty_id: None,
            cursor_boot,
            cursor_offset,
            total_written: None,
            resume_offset: cursor_offset,
            replay_truncated: None,
            replay_truncated_from: None,
            boot_changed: false,
        });
    }

    guard.diagnostics = Some(diagnostics);
    let stream = async_stream::stream! {
        let mut guard = guard;
        let mut upstream = Box::pin(upstream);
        while let Some(result) = upstream.next().await {
            match result {
                Ok(bytes) => {
                    yield Ok::<_, reqwest::Error>(bytes.clone());
                    if let Some(diagnostics) = guard.diagnostics.as_mut() {
                        diagnostics.observe_sse_bytes(&bytes);
                    }
                }
                Err(error) => {
                    if let Some(diagnostics) = guard.diagnostics.as_mut() {
                        diagnostics.finish("upstream_error");
                    }
                    yield Err(error);
                    return;
                }
            }
        }
        if let Some(diagnostics) = guard.diagnostics.as_mut() {
            diagnostics.finish("upstream_eof");
        }
    };
    Body::from_stream(stream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::stream;

    #[tokio::test]
    async fn output_body_without_level_forwards_upstream_bytes_unchanged() {
        let upstream = stream::iter([
            Ok::<_, reqwest::Error>(Bytes::from_static(b"data: {\"output\":")),
            Ok(Bytes::from_static(b"\"secret-free\"}\n\n")),
        ]);
        let census = crate::stream_census::StreamCensus::default();
        let body = output_body(
            upstream,
            None,
            "*".into(),
            None,
            None,
            census.open(
                crate::stream_census::StreamKind::Output,
                crate::stream_census::StreamScope::PeerRelayed {
                    peer_instance_id: "test-peer".into(),
                },
            ),
        );
        assert_eq!(serde_json::to_value(census.snapshot()).unwrap()["live"], 1);
        let received = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        assert_eq!(&received[..], b"data: {\"output\":\"secret-free\"}\n\n");
        let counts = serde_json::to_value(census.snapshot()).unwrap();
        assert_eq!(counts["live"], 0);
        assert_eq!(counts["opened"], 1);
        assert_eq!(counts["closed"], 1);
    }

    #[tokio::test]
    async fn diagnostic_output_body_closes_the_same_guard_on_end_and_disconnect() {
        use crate::stream_census::{StreamCensus, StreamKind, StreamScope};
        let census = StreamCensus::default();
        let open = || {
            census.open(
                StreamKind::Output,
                StreamScope::PeerRelayed {
                    peer_instance_id: "diagnostic-peer".into(),
                },
            )
        };
        let upstream = stream::iter([Ok::<_, reqwest::Error>(Bytes::from_static(
            b"data: {\"tab_id\":\"t\",\"output\":\"x\"}\n\n",
        ))]);
        let body = output_body(
            upstream,
            Some(log::Level::Debug),
            "t".into(),
            Some("t".into()),
            None,
            open(),
        );
        let received = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        assert_eq!(
            &received[..],
            b"data: {\"tab_id\":\"t\",\"output\":\"x\"}\n\n"
        );
        let counts = serde_json::to_value(census.snapshot()).unwrap();
        assert_eq!(counts["live"], 0);
        assert_eq!(counts["opened"], 1);
        assert_eq!(counts["closed"], 1);

        let body = output_body(
            stream::pending::<Result<Bytes, reqwest::Error>>(),
            Some(log::Level::Debug),
            "*".into(),
            None,
            None,
            open(),
        );
        assert_eq!(serde_json::to_value(census.snapshot()).unwrap()["live"], 1);
        drop(body);
        let counts = serde_json::to_value(census.snapshot()).unwrap();
        assert_eq!(counts["live"], 0);
        assert_eq!(counts["opened"], 2);
        assert_eq!(counts["closed"], 2);
    }
}
