//! `ct.host.*` user-gesture actions of a plugin view. The app stamps the plugin
//! id and verifies the gesture; these routes enforce the declaration and media
//! ownership (see `codeterm_commands::commands::plugins::view_host_action`).

use axum::{
    extract::State as AxumState,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use futures_util::StreamExt;
use serde::Deserialize;

use crate::routes::{check_auth, AppState};
use codeterm_commands::commands::plugins::{view_host_action, view_host_download, HostActionRequest};

const DOWNLOAD_CHUNK: usize = 64 * 1024;

/// Refusals are typed results, so they share the success status.
pub async fn host_action(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(request): Json<HostActionRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let svc = state.svc.clone();
    tokio::task::spawn_blocking(move || view_host_action(&svc, request))
        .await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct DownloadRequest {
    plugin_id: String,
    #[serde(default)]
    frame: Option<String>,
    #[serde(default)]
    handle: Option<String>,
}

/// A display name safe for `Content-Disposition`: no separators, controls or quotes, never empty.
fn download_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '/' | '\\' | '"' | ':' | '*' | '?' | '<' | '>' | '|'))
        .take(120)
        .collect();
    let trimmed = cleaned.trim().trim_start_matches('.');
    if trimmed.is_empty() {
        "media".to_string()
    } else {
        trimmed.to_string()
    }
}

/// `saveAs` for the web mirror: streams the plugin's own opened file as an attachment.
/// A refusal answers 409 with the typed `{ ok: false, code, reason }` body.
pub async fn host_action_download(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(request): Json<DownloadRequest>,
) -> Result<Response, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let svc = state.svc.clone();
    let opened = tokio::task::spawn_blocking(move || {
        view_host_download(
            &svc,
            &request.plugin_id,
            request.frame.as_deref(),
            request.handle.as_deref(),
        )
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let media = match opened {
        Ok(media) => media,
        Err(refusal) => return Ok((StatusCode::CONFLICT, Json(refusal)).into_response()),
    };
    let name = download_name(&media.file_name);
    let encoded: String = name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    let file = tokio::fs::File::from_std(media.file);
    let body = futures_util::stream::unfold(file, |mut file| async move {
        use tokio::io::AsyncReadExt;
        let mut chunk = vec![0u8; DOWNLOAD_CHUNK];
        match file.read(&mut chunk).await {
            Ok(0) => None,
            Ok(n) => {
                chunk.truncate(n);
                Some((Ok::<_, std::io::Error>(axum::body::Bytes::from(chunk)), file))
            }
            Err(error) => Some((Err(error), file)),
        }
    })
    .boxed();
    let mut response = axum::body::Body::from_stream(body).into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static("application/octet-stream"));
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Ok(disposition) = HeaderValue::from_str(&format!("attachment; filename*=UTF-8''{encoded}")) {
        headers.insert(header::CONTENT_DISPOSITION, disposition);
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::download_name;

    #[test]
    fn download_names_lose_separators_quotes_and_leading_dots() {
        assert_eq!(download_name("frame.png"), "frame.png");
        assert_eq!(download_name("../../etc/passwd"), "etcpasswd");
        assert_eq!(download_name("..\\a\"b\r\n.mp4"), "ab.mp4");
        assert_eq!(download_name("  "), "media");
        assert_eq!(download_name(".hidden"), "hidden");
    }
}
