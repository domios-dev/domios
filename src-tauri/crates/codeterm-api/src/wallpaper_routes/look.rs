use codeterm_types::config::schema::AppearanceLook;

use super::*;

pub(super) fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/chat-wallpaper/appearance",
        get(read_look).put(apply_look),
    )
}

async fn read_look(
    State(state): State<AppState>,
    Extension(origin): Extension<AuthenticatedOrigin>,
) -> Result<impl IntoResponse, Refusal> {
    require_owner(origin)?;
    Ok(Json(AppearanceLook::from_config(&state.config.lock())))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CopyLook {
    mutating: AppearanceLook,
    expected: AppearanceLook,
}

async fn apply_look(
    State(state): State<AppState>,
    Extension(origin): Extension<AuthenticatedOrigin>,
    Extension(assets): Extension<Arc<WallpaperAssets>>,
    Json(mut copy): Json<CopyLook>,
) -> Result<impl IntoResponse, crate::appearance_routes::ApiRefusal> {
    require_owner(origin).map_err(typed_wallpaper_refusal)?;
    tokio::task::spawn_blocking(move || {
        let _writer = assets.settings_writer.lock();
        validate_wallpaper(&assets, &mut copy.mutating.wallpaper).map_err(owned_refusal)?;
        crate::appearance_routes::validate(&copy.mutating)?;
        let mut stale = false;
        let committed = write_appearance(&state, &assets, |config| {
            if AppearanceLook::from_config(config) != copy.expected {
                stale = true;
                return Err("Machine appearance changed".into());
            }
            copy.mutating.write_to(config);
            config.appearance_active = None;
            Ok(())
        });
        if stale {
            return Err((
                StatusCode::CONFLICT,
                "Machine appearance changed before the copy was applied".to_owned(),
            ));
        }
        committed.map_err(owned_refusal)
    })
    .await
    .map_err(|_| typed_wallpaper_refusal(worker_stopped()))?
    .map_err(crate::appearance_routes::typed_refusal)?;
    Ok(Json(json!({ "ok": true })))
}
