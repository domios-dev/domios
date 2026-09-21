use axum::body::Body;
use futures_util::StreamExt;

use crate::api::output_stream;
use crate::api::stream_diagnostics::{StreamDiagnostics, StreamTabStart};

pub(crate) fn output_body(
    response: reqwest::Response,
    level: Option<log::Level>,
    tab_scope: String,
    stream_tab_id: Option<String>,
    since: Option<&str>,
) -> Body {
    let upstream = response.bytes_stream();
    let Some(level) = level else {
        return Body::from_stream(upstream);
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

    let stream = async_stream::stream! {
        let mut upstream = Box::pin(upstream);
        while let Some(result) = upstream.next().await {
            match result {
                Ok(bytes) => {
                    yield Ok::<_, reqwest::Error>(bytes.clone());
                    diagnostics.observe_sse_bytes(&bytes);
                }
                Err(error) => {
                    diagnostics.finish("upstream_error");
                    yield Err(error);
                    return;
                }
            }
        }
        diagnostics.finish("upstream_eof");
    };
    Body::from_stream(stream)
}
