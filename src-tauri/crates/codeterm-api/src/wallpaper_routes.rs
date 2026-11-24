use std::{path::PathBuf, sync::Arc, time::Duration};

use axum::{
    body::Body,
    extract::{Path, Query, Request, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post, put},
    Extension, Json, Router,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use codeterm_service_core::service::chat_wallpaper::{
    AssetError, WallpaperStore, ASSET_LIMIT, UPLOAD_LIMIT,
};
use codeterm_types::{config::schema::ChatWallpaper, svg::SvgRefusal};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::Semaphore;

use crate::{auth::AuthenticatedOrigin, routes::AppState};

pub(super) type Refusal = (StatusCode, &'static str);

mod draft;
mod look;

pub(super) struct WallpaperAssets {
    store: WallpaperStore,
    upload_slot: Arc<Semaphore>,
    read_slots: Arc<Semaphore>,
    pub(super) settings_writer: parking_lot::Mutex<()>,
    config_path: Result<PathBuf, String>,
    draft: parking_lot::Mutex<Option<draft::Draft>>,
}

impl WallpaperAssets {
    pub(super) fn svg_asset(&self, id: &str) -> Option<Vec<u8>> {
        if !id.ends_with(".svg") {
            return None;
        }
        self.store.read(id).ok().map(|(_, bytes)| bytes)
    }
}

pub(super) fn routes() -> Router<AppState> {
    with_store(
        WallpaperStore::new(codeterm_service_core::paths::data_dir_path()),
        codeterm_service_core::paths::config_path(),
    )
}

fn with_store(store: WallpaperStore, config_path: Result<PathBuf, String>) -> Router<AppState> {
    let assets = Arc::new(WallpaperAssets {
        store,
        upload_slot: Arc::new(Semaphore::new(1)),
        read_slots: Arc::new(Semaphore::new(4)),
        settings_writer: parking_lot::Mutex::new(()),
        config_path,
        draft: Default::default(),
    });
    Router::new()
        .route(
            "/api/serve-file",
            get(super::notification_routes::serve_file),
        )
        .route("/api/chat-wallpaper/assets", post(upload))
        .route("/api/chat-wallpaper/assets/copy/{id}", post(copy_asset))
        .route("/api/chat-wallpaper/assets/encoded", post(upload_encoded))
        .route("/api/chat-wallpaper/assets/{id}", delete(remove))
        .route("/api/chat-wallpaper", put(set))
        .merge(draft::routes())
        .merge(look::routes())
        .merge(crate::appearance_routes::router())
        .route("/api/mesh/chat-wallpaper/assets/{peer}", post(relay_upload))
        .layer(Extension(assets))
}

fn require_owner(origin: AuthenticatedOrigin) -> Result<(), Refusal> {
    if origin == AuthenticatedOrigin::Owner {
        Ok(())
    } else {
        Err((StatusCode::FORBIDDEN, "Owner authorization required"))
    }
}

fn require_wallpaper_writer(
    origin: AuthenticatedOrigin,
) -> Result<(), crate::appearance_routes::ApiRefusal> {
    require_owner(origin).map_err(|(status, _)| {
        (status, Json(json!({
            "code": "wallpaper_owner_required",
            "message": "Have the owner apply this wallpaper from a native proposal card. Read domios docs APPEARANCE, run domios appearance propose, and return its <ds-appearance> block in this chat."
        })))
    })
}

impl WallpaperAssets {
    pub(super) async fn serve(
        self: Arc<Self>,
        origin: AuthenticatedOrigin,
        id: String,
        encoded: bool,
    ) -> Result<Response, Refusal> {
        require_owner(origin)?;
        let permit = self.read_slots.clone().try_acquire_owned().map_err(|_| {
            (
                StatusCode::TOO_MANY_REQUESTS,
                "Wallpaper downloads are busy",
            )
        })?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let (media_type, bytes) = self.store.read(&id).map_err(refusal)?;
            let body = if encoded {
                Json(json!({"media_type": media_type, "data": STANDARD.encode(bytes)}))
                    .into_response()
            } else {
                ([(header::CONTENT_TYPE, media_type)], Body::from(bytes)).into_response()
            };
            let mut response = body;
            response.headers_mut().extend([
                (
                    header::X_CONTENT_TYPE_OPTIONS,
                    header::HeaderValue::from_static("nosniff"),
                ),
                (
                    header::CACHE_CONTROL,
                    header::HeaderValue::from_static("private, no-store"),
                ),
                (
                    header::CONTENT_SECURITY_POLICY,
                    header::HeaderValue::from_static("sandbox; default-src 'none'"),
                ),
            ]);
            Ok(response)
        })
        .await
        .map_err(|_| worker_stopped())?
    }
}

async fn upload(
    Extension(origin): Extension<AuthenticatedOrigin>,
    Extension(assets): Extension<Arc<WallpaperAssets>>,
    request: Request,
) -> Result<impl IntoResponse, Refusal> {
    require_owner(origin)?;
    let media_type = input_type(request.headers())?;
    let permit = try_upload(&assets)?;
    let bytes = receive_with_timeout(request.into_body(), upload_limit(&media_type)?).await?;
    let id = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        assets.store.save(&media_type, &bytes)
    })
    .await
    .map_err(|_| worker_stopped())?
    .map_err(refusal)?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

fn input_type(headers: &HeaderMap) -> Result<String, Refusal> {
    let media_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(|value| value.trim().to_ascii_lowercase())
        .ok_or_else(|| refusal(AssetError::Unsupported))?;
    upload_limit(&media_type)?;
    Ok(media_type)
}

fn try_upload(assets: &WallpaperAssets) -> Result<tokio::sync::OwnedSemaphorePermit, Refusal> {
    assets.upload_slot.clone().try_acquire_owned().map_err(|_| {
        (
            StatusCode::TOO_MANY_REQUESTS,
            "Another wallpaper upload is in progress",
        )
    })
}

fn upload_limit(media_type: &str) -> Result<usize, Refusal> {
    match media_type {
        "image/svg+xml" => Ok(codeterm_types::svg::MAX_CHAT_TILE_BYTES),
        "image/png" | "image/jpeg" | "image/webp" => Ok(UPLOAD_LIMIT),
        _ => Err(refusal(AssetError::Unsupported)),
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EncodedUpload {
    media_type: String,
    data: String,
    copy_id: Option<String>,
}

async fn upload_encoded(
    Extension(origin): Extension<AuthenticatedOrigin>,
    Extension(assets): Extension<Arc<WallpaperAssets>>,
    request: Request,
) -> Result<impl IntoResponse, Refusal> {
    require_owner(origin)?;
    let permit = try_upload(&assets)?;
    let bytes =
        receive_with_timeout(request.into_body(), ASSET_LIMIT.div_ceil(3) * 4 + 512).await?;
    let id = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let upload: EncodedUpload =
            serde_json::from_slice(&bytes).map_err(|_| refusal(AssetError::Invalid))?;
        let limit = transfer_limit(&upload.media_type, upload.copy_id.as_deref())?;
        if upload.data.len() > limit.div_ceil(3) * 4 {
            return Err(refusal(AssetError::TooLarge));
        }
        let bytes = STANDARD
            .decode(upload.data)
            .map_err(|_| refusal(AssetError::Invalid))?;
        if let Some(id) = upload.copy_id {
            assets.store.copy(&id, &bytes).map_err(refusal)?;
            Ok(id)
        } else {
            assets
                .store
                .save(&upload.media_type, &bytes)
                .map_err(refusal)
        }
    })
    .await
    .map_err(|_| worker_stopped())??;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

async fn copy_asset(
    Extension(origin): Extension<AuthenticatedOrigin>,
    Extension(assets): Extension<Arc<WallpaperAssets>>,
    Path(id): Path<String>,
    request: Request,
) -> Result<StatusCode, Refusal> {
    require_owner(origin)?;
    let media_type = input_type(request.headers())?;
    let permit = try_upload(&assets)?;
    let bytes =
        receive_with_timeout(request.into_body(), transfer_limit(&media_type, Some(&id))?).await?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        assets.store.copy(&id, &bytes).map_err(refusal)
    })
    .await
    .map_err(|_| worker_stopped())??;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct CopySource {
    copy_id: Option<String>,
}

fn transfer_limit(media_type: &str, copy_id: Option<&str>) -> Result<usize, Refusal> {
    let format = copy_id.and_then(codeterm_types::config::schema::wallpaper_asset_format);
    match (media_type, copy_id, format) {
        ("image/png", Some(_), Some("png")) => Ok(ASSET_LIMIT),
        ("image/svg+xml", Some(_), Some("svg")) | (_, None, _) => upload_limit(media_type),
        _ => Err(refusal(AssetError::Invalid)),
    }
}

async fn relay_upload(
    State(state): State<AppState>,
    Path(peer): Path<String>,
    Query(copy): Query<CopySource>,
    Extension(origin): Extension<AuthenticatedOrigin>,
    Extension(identity): Extension<crate::auth::CallerIdentity>,
    Extension(assets): Extension<Arc<WallpaperAssets>>,
    request: Request,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    use super::mesh_routes::relay::{mesh_relay_to_peer, PeerRelayCommand};
    let owned = |(code, message): Refusal| (code, message.to_string());
    require_owner(origin).map_err(owned)?;
    let media_type = input_type(request.headers()).map_err(owned)?;
    let permit = try_upload(&assets).map_err(owned)?;
    let (parts, body) = request.into_parts();
    let bytes = receive_with_timeout(
        body,
        transfer_limit(&media_type, copy.copy_id.as_deref()).map_err(owned)?,
    )
    .await
    .map_err(owned)?;
    let body = tokio::task::spawn_blocking(move || {
        json!({
            "media_type": media_type,
            "data": STANDARD.encode(bytes),
            "copy_id": copy.copy_id,
        })
    })
    .await
    .map_err(|_| owned(worker_stopped()))?;
    let result = mesh_relay_to_peer(
        State(state),
        Extension(identity),
        parts.headers,
        Json(PeerRelayCommand {
            target_peer_id: None,
            target_instance_id: Some(peer),
            method: "POST".into(),
            path: "/api/chat-wallpaper/assets/encoded".into(),
            body,
        }),
    )
    .await;
    drop(permit);
    result
}

async fn receive_with_timeout(body: Body, limit: usize) -> Result<Vec<u8>, Refusal> {
    tokio::time::timeout(Duration::from_secs(20), receive(body, limit))
        .await
        .map_err(|_| (StatusCode::REQUEST_TIMEOUT, "Wallpaper upload timed out"))?
}

async fn receive(body: Body, limit: usize) -> Result<Vec<u8>, Refusal> {
    let mut stream = body.into_data_stream();
    let mut bytes = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| {
            (
                StatusCode::BAD_REQUEST,
                "Could not read the wallpaper upload",
            )
        })?;
        if chunk.len() > limit - bytes.len() {
            return Err(refusal(AssetError::TooLarge));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn remove(
    State(state): State<AppState>,
    Extension(origin): Extension<AuthenticatedOrigin>,
    Extension(assets): Extension<Arc<WallpaperAssets>>,
    Path(id): Path<String>,
) -> Result<impl IntoResponse, Refusal> {
    require_owner(origin)?;
    tokio::task::spawn_blocking(move || {
        let _writer = assets.settings_writer.lock();
        let in_use = {
            let config = state.config.lock();
            let matches = |paper: Option<&ChatWallpaper>| {
                paper.and_then(|wallpaper| wallpaper.asset.as_deref()) == Some(id.as_str())
            };
            matches(config.chat_wallpaper.as_ref())
                || config.appearance_proposals.iter().any(|proposal| {
                    matches(proposal.look.wallpaper.as_ref())
                        || matches(proposal.previous.wallpaper.as_ref())
                })
        };
        if in_use {
            return Err((
                StatusCode::CONFLICT,
                "This wallpaper is still used by the current look or an Appearance Lab proposal",
            ));
        }
        assets.store.remove(&id).map_err(refusal)
    })
    .await
    .map_err(|_| worker_stopped())??;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Setting {
    wallpaper: Option<ChatWallpaper>,
    expected_wallpaper: Option<ChatWallpaper>,
}

async fn set(
    State(state): State<AppState>,
    Extension(origin): Extension<AuthenticatedOrigin>,
    Extension(assets): Extension<Arc<WallpaperAssets>>,
    Json(setting): Json<Setting>,
) -> Result<impl IntoResponse, crate::appearance_routes::ApiRefusal> {
    require_wallpaper_writer(origin)?;
    tokio::task::spawn_blocking(move || {
        let _writer = assets.settings_writer.lock();
        apply_setting(&state, &assets, setting, false)
    })
    .await
    .map_err(|_| typed_wallpaper_refusal(worker_stopped()))?
    .map_err(crate::appearance_routes::typed_refusal)?;
    Ok(Json(json!({ "ok": true })))
}

fn apply_setting(
    state: &AppState,
    assets: &WallpaperAssets,
    mut setting: Setting,
    agent: bool,
) -> Result<(), (StatusCode, String)> {
    validate_wallpaper(assets, &mut setting.wallpaper).map_err(owned_refusal)?;
    let mut failure = None;
    let mut changed = false;
    let config = update_appearance(state, assets, |config| {
        if setting
            .expected_wallpaper
            .as_ref()
            .is_some_and(|expected| config.chat_wallpaper.as_ref() != Some(expected))
        {
            let message = "The wallpaper changed before this update";
            failure = Some((StatusCode::CONFLICT, message.to_owned()));
            return Err(message.into());
        }
        let mut look = codeterm_types::config::schema::AppearanceLook::from_config(config);
        changed = look.wallpaper != setting.wallpaper;
        look.wallpaper = setting.wallpaper;
        crate::appearance_routes::validate(&look).map_err(|(status, message)| {
            failure = Some((status, message.clone()));
            message
        })?;
        look.write_to(config);
        Ok(())
    })
    .map_err(|error| failure.unwrap_or((StatusCode::INTERNAL_SERVER_ERROR, error)))?;
    if changed && agent {
        crate::appearance_routes::emit_change(state, &config, None, false);
    }
    publish_config(state, &config);
    Ok(())
}

fn owned_refusal((status, message): Refusal) -> (StatusCode, String) {
    (status, message.to_owned())
}

fn typed_wallpaper_refusal(refusal: Refusal) -> crate::appearance_routes::ApiRefusal {
    crate::appearance_routes::typed_refusal(owned_refusal(refusal))
}

pub(super) fn validate_wallpaper(
    assets: &WallpaperAssets,
    wallpaper: &mut Option<ChatWallpaper>,
) -> Result<(), Refusal> {
    if let Some(wallpaper) = wallpaper {
        wallpaper
            .validate()
            .map_err(|err| (StatusCode::BAD_REQUEST, err))?;
        if let Some(id) = &wallpaper.asset {
            assets.store.read(id).map_err(refusal)?;
        }
    }
    Ok(())
}

fn write_appearance(
    state: &AppState,
    assets: &WallpaperAssets,
    update: impl FnOnce(&mut codeterm_types::config::schema::AppConfig) -> Result<(), String>,
) -> Result<(), Refusal> {
    let config = update_appearance(state, assets, update).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Wallpaper setting could not be saved",
        )
    })?;
    publish_config(state, &config);
    Ok(())
}

pub(super) fn publish_config(state: &AppState, config: &codeterm_types::config::schema::AppConfig) {
    let payload = serde_json::to_value(config).unwrap_or_default();
    state.svc.ui.emit(
        "config-changed",
        super::config_routes::redact_rest_config(payload),
    );
}

pub(super) fn update_appearance(
    state: &AppState,
    assets: &WallpaperAssets,
    update: impl FnOnce(&mut codeterm_types::config::schema::AppConfig) -> Result<(), String>,
) -> Result<codeterm_types::config::schema::AppConfig, String> {
    let path = assets
        .config_path
        .as_ref()
        .map_err(|_| "Wallpaper settings path is unavailable".to_string())?;
    codeterm_service_core::config::update_config_and_apply_at(path, &state.config, update)
}

fn refusal(error: AssetError) -> Refusal {
    match error {
        AssetError::TooLarge => (
            StatusCode::PAYLOAD_TOO_LARGE,
            "The file is too large. Choose a smaller SVG or reduce the image's size or dimensions.",
        ),
        AssetError::Unsupported => (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "This file type is unsupported. Choose a PNG, JPEG, WebP, or SVG file.",
        ),
        AssetError::Invalid => (
            StatusCode::BAD_REQUEST,
            "The file could not be read as a supported static image. Export it again as PNG, JPEG, WebP, or SVG.",
        ),
        AssetError::Svg(reason) => svg_refusal(reason),
        AssetError::Full => (
            StatusCode::CONFLICT,
            "Remove a saved wallpaper before uploading another",
        ),
        AssetError::Missing => (StatusCode::NOT_FOUND, "Wallpaper unavailable"),
        AssetError::Storage(error) => {
            log::debug!("Wallpaper storage refused: {}", error.kind());
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Wallpaper storage unavailable",
            )
        }
    }
}

fn svg_refusal(reason: SvgRefusal) -> Refusal {
    let message = match reason {
        SvgRefusal::TooLarge => return (
            StatusCode::PAYLOAD_TOO_LARGE,
            "The SVG is too large. Choose an SVG up to 64 KB.",
        ),
        SvgRefusal::Empty => "The SVG is empty. Export a tile with shapes or paths.",
        SvgRefusal::InvalidXml => "The SVG contains invalid XML. Export the tile again as SVG.",
        SvgRefusal::InvalidStructure => "The SVG has an invalid structure. Export the tile with a single SVG root.",
        SvgRefusal::Script => "It contains a script. Remove all script elements and save the SVG again.",
        SvgRefusal::EventHandler => "It contains an event handler. Remove event attributes from the SVG.",
        SvgRefusal::ExternalReference => "It contains an external reference. Remove links and embedded files.",
        SvgRefusal::Reference => "It contains a reference to another element. Replace references with plain shapes.",
        SvgRefusal::Style => "It contains styles. Apply strokes and fills directly to the shapes.",
        SvgRefusal::UnsupportedElement => "It contains an unsupported SVG element. Use plain shapes and paths.",
        SvgRefusal::UnsupportedAttribute => "It contains an unsupported SVG attribute. Export as a plain SVG.",
        SvgRefusal::InvalidAttribute => "It contains an invalid SVG attribute value. Correct the value or export the SVG again.",
        SvgRefusal::UnsupportedMarkup => "It contains unsupported text or XML markup. Export the tile as plain SVG with paths and shapes.",
        SvgRefusal::MissingViewBox => "The SVG has no viewBox. Export it with a square viewBox.",
        SvgRefusal::InvalidViewBox => "The SVG has an invalid viewBox. Use four numbers with positive width and height.",
        SvgRefusal::NonSquare => "The SVG is not square. Export it with equal viewBox width and height.",
        SvgRefusal::TooDeep => "The SVG has too many nested groups. Flatten the groups and export it again.",
    };
    (StatusCode::BAD_REQUEST, message)
}

fn worker_stopped() -> Refusal {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "Wallpaper worker stopped",
    )
}

#[cfg(test)]
mod tests;
