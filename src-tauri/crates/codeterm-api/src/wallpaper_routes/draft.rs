use std::time::Instant;

use axum::extract::DefaultBodyLimit;
use codeterm_service_core::service::{
    general_agent::{is_general_agent, GENERAL_ALIAS},
    tab_credentials::TabCaller,
};
use codeterm_types::config::schema::{ChatBackground, ChatPattern};
use codeterm_types::svg::MAX_CHAT_TILE_BYTES;
use uuid::Uuid;

use super::*;
use crate::auth::CallerIdentity;

const LIFETIME: Duration = Duration::from_secs(10 * 60);
const SVG_JSON_LIMIT: usize = MAX_CHAT_TILE_BYTES * 2 + 128;

pub(super) struct Draft {
    id: Uuid,
    caller: TabCaller,
    expires: Instant,
    previous: Option<ChatWallpaper>,
}

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/chat-wallpaper/drafts", post(issue))
        .route(
            "/api/chat-wallpaper/apply-svg",
            post(apply_svg).layer(DefaultBodyLimit::max(SVG_JSON_LIMIT)),
        )
        .route(
            "/api/chat-wallpaper/drafts/{request}",
            post(submit).layer(DefaultBodyLimit::max(SVG_JSON_LIMIT)),
        )
}

fn bound_general(state: &AppState) -> Option<TabCaller> {
    let mux = state.svc.mux.read();
    let id = mux.resolve_tab_id(GENERAL_ALIAS)?;
    let tab = mux.get_tab(&id)?;
    if !tab.protected || !is_general_agent(tab) {
        return None;
    }
    Some(TabCaller {
        tab_id: id,
        binding_nonce: tab.session_binding_nonce.clone()?,
    })
}

async fn issue(
    State(state): State<AppState>,
    Extension(origin): Extension<AuthenticatedOrigin>,
    Extension(assets): Extension<Arc<WallpaperAssets>>,
) -> Result<impl IntoResponse, Refusal> {
    require_owner(origin)?;
    let caller = bound_general(&state).ok_or((StatusCode::CONFLICT, "General is unavailable"))?;
    let id = Uuid::new_v4();
    let tab_id = caller.tab_id.clone();
    let previous = state.config.lock().chat_wallpaper.clone();
    *assets.draft.lock() = Some(Draft {
        id,
        caller,
        expires: Instant::now() + LIFETIME,
        previous,
    });
    Ok(Json(json!({ "request": id, "tab_id": tab_id })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SvgBody {
    svg: String,
}

async fn apply_svg(
    State(state): State<AppState>,
    Extension(origin): Extension<AuthenticatedOrigin>,
    Extension(assets): Extension<Arc<WallpaperAssets>>,
    Json(body): Json<SvgBody>,
) -> Result<impl IntoResponse, crate::appearance_routes::ApiRefusal> {
    require_wallpaper_writer(origin)?;
    apply(state, assets, body.svg, None).await
}

async fn submit(
    State(state): State<AppState>,
    Extension(origin): Extension<AuthenticatedOrigin>,
    Extension(identity): Extension<CallerIdentity>,
    Extension(assets): Extension<Arc<WallpaperAssets>>,
    Path(request): Path<Uuid>,
    Json(body): Json<SvgBody>,
) -> Result<impl IntoResponse, crate::appearance_routes::ApiRefusal> {
    let CallerIdentity::Tab(caller) = identity else {
        return Err(typed_wallpaper_refusal(invalid_draft()));
    };
    if origin != AuthenticatedOrigin::Agent {
        return Err(typed_wallpaper_refusal(invalid_draft()));
    }
    apply(state, assets, body.svg, Some((request, caller))).await
}

async fn apply(
    state: AppState,
    assets: Arc<WallpaperAssets>,
    svg: String,
    request: Option<(Uuid, TabCaller)>,
) -> Result<impl IntoResponse, crate::appearance_routes::ApiRefusal> {
    let permit = try_upload(&assets).map_err(typed_wallpaper_refusal)?;
    let id = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let _writer = assets.settings_writer.lock();
        let mut draft = assets.draft.lock();
        if let Some((id, caller)) = &request {
            let draft = draft
                .as_ref()
                .filter(|draft| {
                    draft.id == *id && draft.caller == *caller && draft.expires > Instant::now()
                })
                .ok_or_else(|| owned_refusal(invalid_draft()))?;
            if bound_general(&state).as_ref() != Some(caller) {
                return Err(owned_refusal(invalid_draft()));
            }
            if state.config.lock().chat_wallpaper != draft.previous {
                return Err((
                    StatusCode::CONFLICT,
                    "The wallpaper changed while General was drawing".to_owned(),
                ));
            }
        }
        let id = assets
            .store
            .save("image/svg+xml", svg.as_bytes())
            .map_err(|error| owned_refusal(refusal(error)))?;
        let wallpaper = ChatWallpaper {
            background: ChatBackground {
                pattern: ChatPattern::Svg,
                ..Default::default()
            },
            asset: Some(id.clone()),
            asset_label: Some("General’s wallpaper.svg".into()),
            palette: None,
            fill: None,
            fill_strength: None,
        };
        apply_setting(
            &state,
            &assets,
            Setting {
                wallpaper: Some(wallpaper),
                expected_wallpaper: None,
            },
            request.is_some(),
        )?;
        *draft = None;
        Ok::<_, (StatusCode, String)>(id)
    })
    .await
    .map_err(|_| typed_wallpaper_refusal(worker_stopped()))?
    .map_err(crate::appearance_routes::typed_refusal)?;
    Ok(Json(json!({ "id": id })))
}

fn invalid_draft() -> Refusal {
    (
        StatusCode::FORBIDDEN,
        "This wallpaper request requires the assigned General session and an active owner request",
    )
}
