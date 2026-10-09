//! Local desk state a mesh peer never receives, whichever route answers it.

use axum::body::Body;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::Value;

const LOCAL_SURFACE_FIELDS: &[&str] = &["resume_url", "resume_profile"];
const MARKER: &[u8] = b"resume_";

/// Strips surface resume points from any surface backing in `value`; true when one was removed.
pub(crate) fn redact_for_peer(value: &mut Value) -> bool {
    match value {
        Value::Object(map) => {
            let mut changed = false;
            if map.contains_key("plugin_id") && map.contains_key("surface_id") {
                for field in LOCAL_SURFACE_FIELDS {
                    changed |= map.remove(*field).is_some();
                }
            }
            for child in map.values_mut() {
                changed |= redact_for_peer(child);
            }
            changed
        }
        Value::Array(items) => items
            .iter_mut()
            .fold(false, |changed, item| redact_for_peer(item) | changed),
        _ => false,
    }
}

fn is_json(response: &Response) -> bool {
    response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("application/json"))
}

/// A JSON answer to a peer request, re-encoded only when it carried a resume point.
pub(crate) async fn redact_peer_response(response: Response) -> Response {
    if !is_json(&response) {
        return response;
    }
    let (mut parts, body) = response.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, usize::MAX).await else {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    if !bytes.windows(MARKER.len()).any(|window| window == MARKER) {
        return Response::from_parts(parts, Body::from(bytes));
    }
    let Ok(mut value) = serde_json::from_slice::<Value>(&bytes) else {
        return Response::from_parts(parts, Body::from(bytes));
    };
    if !redact_for_peer(&mut value) {
        return Response::from_parts(parts, Body::from(bytes));
    }
    parts.headers.remove(header::CONTENT_LENGTH);
    let body = serde_json::to_vec(&value).unwrap_or_default();
    Response::from_parts(parts, Body::from(body))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn peer_redaction_strips_resume_points_from_tab_and_leaf_backings() {
        let mut value = json!({
            "tabs": [{
                "id": "t1",
                "backing": {
                    "kind": "surface",
                    "plugin_id": "browser",
                    "surface_id": "browser",
                    "instance_id": "i",
                    "resume_url": "https://mail.example.com/",
                    "resume_profile": "work"
                }
            }],
            "split_tree": { "surface": {
                "plugin_id": "browser",
                "surface_id": "browser",
                "instance_id": "i",
                "resume_url": "https://mail.example.com/"
            }}
        });
        assert!(redact_for_peer(&mut value));
        let text = value.to_string();
        assert!(!text.contains("resume_"), "{text}");
        assert_eq!(value["tabs"][0]["backing"]["instance_id"], "i");
        assert!(!redact_for_peer(&mut value));
    }

    #[test]
    fn peer_redaction_leaves_unrelated_resume_keys_alone() {
        let mut value = json!({ "resume_url": "kept", "items": [1, "two"] });
        assert!(!redact_for_peer(&mut value));
        assert_eq!(value["resume_url"], "kept");
    }

    #[tokio::test]
    async fn peer_redaction_rewrites_only_json_bodies_that_carry_a_resume_point() {
        let leaky = json!({ "backing": {
            "plugin_id": "browser",
            "surface_id": "browser",
            "resume_url": "https://a.dev/"
        }});
        let response = axum::Json(leaky).into_response();
        let redacted = redact_peer_response(response).await;
        let bytes = axum::body::to_bytes(redacted.into_body(), usize::MAX)
            .await
            .unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("resume_url"));

        let plain = (
            [(header::CONTENT_TYPE, "text/plain")],
            "resume_url: https://a.dev/",
        )
            .into_response();
        let untouched = redact_peer_response(plain).await;
        let bytes = axum::body::to_bytes(untouched.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(&bytes[..], b"resume_url: https://a.dev/");
    }
}
