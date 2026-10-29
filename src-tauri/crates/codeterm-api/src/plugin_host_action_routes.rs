//! `ct.host.*` user-gesture actions of a plugin view. The app stamps the plugin
//! id and verifies the gesture; these routes enforce the declaration and media
//! ownership (see `codeterm_commands::commands::plugins::view_host_action`).

use axum::{
    extract::{Request, State as AxumState},
    handler::Handler,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{post, MethodRouter},
    Json,
};
use futures_util::StreamExt;
use serde::Deserialize;

use crate::auth::CallerIdentity;
use crate::routes::{check_auth, AppState};
use codeterm_commands::commands::plugins::{
    view_host_action, view_host_download, HostActionRequest,
};

const DOWNLOAD_CHUNK: usize = 64 * 1024;

/// Only the local owner (the desktop app, or the owner's own web session) may act for a view.
/// A tab, peer or service credential could otherwise name any plugin, frame, handle and
/// destination itself and skip the app's gesture and save dialog.
pub async fn require_local_owner(request: Request, next: Next) -> Response {
    if matches!(
        request.extensions().get::<CallerIdentity>(),
        Some(CallerIdentity::Local)
    ) {
        next.run(request).await
    } else {
        (
            StatusCode::FORBIDDEN,
            "Plugin view host actions are local-owner only",
        )
            .into_response()
    }
}

/// `POST` behind [`require_local_owner`], so a refused caller never reaches the handler.
pub fn owner_post<H, T, S>(handler: H) -> MethodRouter<S>
where
    H: Handler<T, S>,
    T: 'static,
    S: Clone + Send + Sync + 'static,
{
    post(handler).route_layer(middleware::from_fn(require_local_owner))
}

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
        .filter(|c| {
            !c.is_control() && !matches!(c, '/' | '\\' | '"' | ':' | '*' | '?' | '<' | '>' | '|')
        })
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
                Some((
                    Ok::<_, std::io::Error>(axum::body::Bytes::from(chunk)),
                    file,
                ))
            }
            Err(error) => Some((Err(error), file)),
        }
    })
    .boxed();
    let mut response = axum::body::Body::from_stream(body).into_response();
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if let Ok(disposition) =
        HeaderValue::from_str(&format!("attachment; filename*=UTF-8''{encoded}"))
    {
        headers.insert(header::CONTENT_DISPOSITION, disposition);
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::{download_name, owner_post, CallerIdentity};
    use axum::{body::Body, http::Request, http::StatusCode, Extension, Router};
    use codeterm_service_core::mesh::types::AccessLevel;
    use codeterm_service_core::service::tab_credentials::TabCaller;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use tower::ServiceExt;

    #[tokio::test]
    async fn host_action_routes_admit_only_the_local_owner_before_the_handler() {
        let callers = [
            (Some(CallerIdentity::Local), StatusCode::OK),
            (
                Some(CallerIdentity::Tab(TabCaller {
                    tab_id: "agent".into(),
                    binding_nonce: "nonce".into(),
                })),
                StatusCode::FORBIDDEN,
            ),
            (
                Some(CallerIdentity::Peer {
                    instance_id: "peer".into(),
                    access_level: AccessLevel::ReadOnly,
                    relay_protocol_version: 0,
                }),
                StatusCode::FORBIDDEN,
            ),
            (
                Some(CallerIdentity::ServicePush {
                    service_id: "svc".into(),
                }),
                StatusCode::FORBIDDEN,
            ),
            (None, StatusCode::FORBIDDEN),
        ];
        for (caller, expected) in callers {
            let reached = Arc::new(AtomicBool::new(false));
            let seen = reached.clone();
            let mut router: Router = Router::new().route(
                "/api/plugins/view-host-action",
                owner_post(move || async move {
                    seen.store(true, Ordering::SeqCst);
                    StatusCode::OK
                }),
            );
            let label = format!("{caller:?}");
            if let Some(caller) = caller {
                router = router.layer(Extension(caller));
            }
            let response = router
                .oneshot(
                    Request::post("/api/plugins/view-host-action")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), expected, "{label}");
            assert_eq!(
                reached.load(Ordering::SeqCst),
                expected == StatusCode::OK,
                "{label}"
            );
        }
    }

    #[test]
    fn download_names_lose_separators_quotes_and_leading_dots() {
        assert_eq!(download_name("frame.png"), "frame.png");
        assert_eq!(download_name("../../etc/passwd"), "etcpasswd");
        assert_eq!(download_name("..\\a\"b\r\n.mp4"), "ab.mp4");
        assert_eq!(download_name("  "), "media");
        assert_eq!(download_name(".hidden"), "hidden");
    }
}
