use crate::{auth::CallerIdentity, routes::AppState};
use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use codeterm_plugin::media_delivery::{self as media, MediaRefusal};
use serde::Deserialize;
type Error = (StatusCode, &'static str);
fn denied(_: MediaRefusal) -> Error {
    (StatusCode::NOT_FOUND, "media unavailable")
}
fn owner(caller: Option<&Extension<CallerIdentity>>) -> Result<(), Error> {
    if matches!(caller.map(|value| &value.0), Some(CallerIdentity::Local)) {
        Ok(())
    } else {
        Err((StatusCode::FORBIDDEN, "local owner required"))
    }
}
fn current_provider(
    state: &AppState,
    plugin: &str,
    frame: &str,
) -> Result<std::sync::Arc<dyn codeterm_plugin::view_host::ViewProvider>, Error> {
    let provider = media::frame_provider(plugin, frame).map_err(denied)?;
    let current = state
        .svc
        .try_ext::<codeterm_plugin::view_host::ViewRegistryHandle>()
        .and_then(|registry| registry.get(plugin))
        .ok_or_else(|| denied(MediaRefusal::Unknown))?;
    if !std::sync::Arc::ptr_eq(&provider, &current) {
        media::revoke_frame(plugin, frame);
        return Err(denied(MediaRefusal::Expired));
    }
    Ok(provider)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Register {
    id: String,
    view_frame: String,
}
pub async fn register(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
    Path((plugin, frame)): Path<(String, String)>,
    Json(request): Json<Register>,
) -> Result<Response, Error> {
    owner(caller.as_ref())?;
    if !media::valid_token(&frame)
        || !media::valid_token(&request.view_frame)
        || request.id.is_empty()
        || request.id.len() > 256
    {
        return Err((StatusCode::BAD_REQUEST, "invalid media request"));
    }
    let provider = current_provider(&state, &plugin, &frame)?;
    let (registered_provider, context) =
        media::view_frame(&plugin, &request.view_frame).map_err(denied)?;
    if !std::sync::Arc::ptr_eq(&provider, &registered_provider)
        || media::frame_context(&plugin, &frame).map_err(denied)? != context
    {
        return Err(denied(MediaRefusal::Expired));
    }
    state
        .svc
        .validate_plugin_view_context(&context)
        .map_err(|_| denied(MediaRefusal::Expired))?;
    let source = provider
        .media_file(&request.id, &context)
        .await
        .map_err(|_| denied(MediaRefusal::Unknown))?;
    current_provider(&state, &plugin, &frame)?;
    state
        .svc
        .validate_plugin_view_context(&context)
        .map_err(|_| denied(MediaRefusal::Expired))?;
    let target_plugin = plugin.clone();
    let target_frame = frame.clone();
    let target_context = context.clone();
    let result = tokio::task::spawn_blocking(move || {
        media::register_context_media(&target_plugin, &target_frame, &target_context, source)
    })
    .await
    .map_err(|_| denied(MediaRefusal::Unknown))?
    .map_err(denied)?;
    if state.svc.validate_plugin_view_context(&context).is_err() {
        media::revoke_context_media(&plugin, &frame, &context, &result.handle);
        return Err(denied(MediaRefusal::Expired));
    }
    let mut response = Json(result).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}
pub async fn revoke(
    caller: Option<Extension<CallerIdentity>>,
    Path((plugin, frame)): Path<(String, String)>,
) -> Result<StatusCode, Error> {
    owner(caller.as_ref())?;
    media::revoke_frame(&plugin, &frame);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RevokeView {
    plugin_id: String,
    registration: String,
}

pub async fn revoke_view(
    caller: Option<Extension<CallerIdentity>>,
    Json(request): Json<RevokeView>,
) -> Result<StatusCode, Error> {
    owner(caller.as_ref())?;
    if !media::valid_plugin_id(&request.plugin_id) || !media::valid_token(&request.registration) {
        return Err((StatusCode::BAD_REQUEST, "invalid view disposal"));
    }
    media::revoke_frame(&request.plugin_id, &request.registration);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ViewRegistrationScope {
    pub surface: Option<String>,
    pub instance: Option<String>,
    pub tab: Option<String>,
    pub owner: Option<String>,
    pub workspace: Option<String>,
    pub team: Option<String>,
    pub provider_registration: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ReregisterView {
    plugin_id: String,
    registration: String,
    scope: ViewRegistrationScope,
}

pub async fn reregister_view(
    State(state): State<AppState>,
    caller: Option<Extension<CallerIdentity>>,
    Json(request): Json<ReregisterView>,
) -> Result<Json<serde_json::Value>, Error> {
    owner(caller.as_ref())?;
    if !media::valid_token(&request.registration) {
        return Err((StatusCode::BAD_REQUEST, "invalid view registration"));
    }
    let context = state
        .svc
        .plugin_view_context(
            &request.plugin_id,
            request.scope.surface.as_deref(),
            request.scope.instance.as_deref(),
            request.scope.tab.as_deref(),
        )
        .map_err(|_| (StatusCode::FORBIDDEN, "view scope changed"))?;
    if context.owner != request.scope.owner
        || context.workspace != request.scope.workspace
        || context.team != request.scope.team
    {
        return Err((StatusCode::FORBIDDEN, "view scope changed"));
    }
    let registry = state
        .svc
        .ext::<codeterm_plugin::view_host::ViewRegistryHandle>();
    if registry.registration_id(&request.plugin_id).as_deref()
        != Some(request.scope.provider_registration.as_str())
    {
        return Err((StatusCode::FORBIDDEN, "view provider changed"));
    }
    let provider = registry
        .get(&request.plugin_id)
        .ok_or((StatusCode::NOT_FOUND, "view unavailable"))?;
    let registered = media::reregister_frame(&provider, registry, context, &request.registration)
        .map_err(denied)?;
    if registry.registration_id(&request.plugin_id).as_deref()
        != Some(request.scope.provider_registration.as_str())
    {
        media::revoke_view_frame(&request.plugin_id, &registered.view_lease);
        return Err((StatusCode::FORBIDDEN, "view provider changed"));
    }
    Ok(Json(serde_json::json!({"viewLease":registered.view_lease})))
}

/// Only these exact GET/HEAD paths bypass owner authentication; the opaque live pair authenticates them.
pub(crate) fn is_playback_path(method: &Method, path: &str) -> bool {
    if *method != Method::GET && *method != Method::HEAD {
        return false;
    }
    let parts: Vec<_> = path.split('/').collect();
    parts.len() == 7
        && parts[1] == "api"
        && parts[2] == "plugins"
        && media::valid_plugin_id(parts[3])
        && parts[4] == "media"
        && media::valid_token(parts[5])
        && media::valid_token(parts[6])
}

#[derive(Debug, PartialEq, Eq)]
struct ByteRange {
    start: u64,
    end: u64,
    partial: bool,
}
fn byte_range(value: Option<&str>, size: u64) -> Result<ByteRange, ()> {
    let Some(value) = value else {
        return Ok(ByteRange {
            start: 0,
            end: size.checked_sub(1).ok_or(())?,
            partial: false,
        });
    };
    if value.len() > 128 {
        return Err(());
    }
    let value = value.strip_prefix("bytes=").ok_or(())?;
    let (start, end) = value.split_once('-').ok_or(())?;
    let number = |text: &str| {
        if !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) {
            text.parse::<u64>().map_err(|_| ())
        } else {
            Err(())
        }
    };
    let (start, end) = if start.is_empty() {
        let suffix = number(end)?;
        if suffix == 0 {
            return Err(());
        }
        (size.saturating_sub(suffix), size.checked_sub(1).ok_or(())?)
    } else {
        let start = number(start)?;
        let end = if end.is_empty() {
            size.checked_sub(1).ok_or(())?
        } else {
            number(end)?.min(size.saturating_sub(1))
        };
        (start, end)
    };
    if start > end || start >= size {
        return Err(());
    }
    Ok(ByteRange {
        start,
        end,
        partial: true,
    })
}

pub async fn playback(
    State(state): State<AppState>,
    Path((plugin, frame, handle)): Path<(String, String, String)>,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, Error> {
    if !media::valid_token(&frame) || !media::valid_token(&handle) {
        return Err(denied(MediaRefusal::Unknown));
    }
    current_provider(&state, &plugin, &frame)?;
    let source=tokio::task::spawn_blocking(move ||media::playback(&plugin,&frame,&handle)).await.map_err(|_|denied(MediaRefusal::Unknown))?.map_err(denied)?;
    playback_response(source, method, headers)
}

trait MediaReader: Clone + Send + Sync + 'static {
    fn size(&self) -> u64;
    fn content_type(&self) -> String;
    fn read(&self, offset: u64, length: usize) -> std::io::Result<Vec<u8>>;
}
impl MediaReader for media::Playback {
    fn size(&self) -> u64 {
        self.size
    }
    fn content_type(&self) -> String {
        self.content_type.clone()
    }
    fn read(&self, offset: u64, length: usize) -> std::io::Result<Vec<u8>> {
        media::Playback::read(self, offset, length)
    }
}
fn playback_response(
    source: impl MediaReader,
    method: Method,
    headers: HeaderMap,
) -> Result<Response, Error> {
    let size = source.size();
    let ranges = headers.get_all(header::RANGE).iter().collect::<Vec<_>>();
    let range = if ranges.len() > 1 {
        Err(())
    } else {
        ranges
            .first()
            .map(|value| value.to_str().map_err(|_| ()))
            .transpose()
            .and_then(|value| byte_range(value, size))
    };
    let Ok(range) = range else {
        let mut response = StatusCode::RANGE_NOT_SATISFIABLE.into_response();
        response.headers_mut().insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes */{}", size))
                .map_err(|_| denied(MediaRefusal::Unknown))?,
        );
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        return Ok(response);
    };
    let content_type = source.content_type();
    let length = range.end - range.start + 1;
    let body = if method == Method::HEAD {
        Body::empty()
    } else {
        let mut offset = range.start;
        let end = range.end;
        Body::from_stream(async_stream::stream! {
            while offset <= end {
                let count = (end - offset + 1).min(65536) as usize;
                let reader = source.clone();
                let chunk = match tokio::task::spawn_blocking(move || reader.read(offset, count)).await {
                    Ok(value) => value,
                    Err(error) => Err(std::io::Error::other(error)),
                };
                match chunk {
                    Ok(chunk) => { offset += chunk.len() as u64; yield Ok::<_, std::io::Error>(axum::body::Bytes::from(chunk)); }
                    Err(error) => { yield Err(error); break; }
                }
            }
        })
    };
    let mut response = Response::new(body);
    *response.status_mut() = if range.partial {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&content_type).map_err(|_| denied(MediaRefusal::Unknown))?,
    );
    headers.insert(
        header::CONTENT_LENGTH,
        HeaderValue::from_str(&length.to_string()).map_err(|_| denied(MediaRefusal::Unknown))?,
    );
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        "cross-origin-resource-policy",
        HeaderValue::from_static("cross-origin"),
    );
    if range.partial {
        headers.insert(
            header::CONTENT_RANGE,
            HeaderValue::from_str(&format!("bytes {}-{}/{}", range.start, range.end, size))
                .map_err(|_| denied(MediaRefusal::Unknown))?,
        );
    }
    Ok(response)
}
#[cfg(test)]
mod tests;
