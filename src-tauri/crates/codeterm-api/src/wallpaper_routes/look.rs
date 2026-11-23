use codeterm_types::config::schema::{AppConfig, ThemeConfig};
use serde::Serialize;

use super::*;

#[derive(Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Appearance {
    theme: ThemeConfig,
    wallpaper: Option<ChatWallpaper>,
}

impl From<&AppConfig> for Appearance {
    fn from(config: &AppConfig) -> Self {
        Self {
            theme: config.theme.clone(),
            wallpaper: config.chat_wallpaper.clone(),
        }
    }
}

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
    Ok(Json(Appearance::from(&*state.config.lock())))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CopyLook {
    mutating: Appearance,
    expected: Appearance,
}

async fn apply_look(
    State(state): State<AppState>,
    Extension(origin): Extension<AuthenticatedOrigin>,
    Extension(assets): Extension<Arc<WallpaperAssets>>,
    Json(mut copy): Json<CopyLook>,
) -> Result<impl IntoResponse, Refusal> {
    require_owner(origin)?;
    if !safe_theme(&copy.mutating.theme) {
        return Err((
            StatusCode::BAD_REQUEST,
            "Copy a theme with a valid ID and hexadecimal colors",
        ));
    }
    tokio::task::spawn_blocking(move || {
        let _writer = assets.settings_writer.lock();
        validate_wallpaper(&assets, &mut copy.mutating.wallpaper)?;
        let mut stale = false;
        let committed = write_appearance(&state, &assets, |config| {
            if Appearance::from(&*config) != copy.expected {
                stale = true;
                return Err("Machine appearance changed".into());
            }
            config.theme = copy.mutating.theme;
            config.chat_wallpaper = copy.mutating.wallpaper;
            Ok(())
        });
        if stale {
            return Err((
                StatusCode::CONFLICT,
                "Machine appearance changed before the copy was applied",
            ));
        }
        committed
    })
    .await
    .map_err(|_| worker_stopped())??;
    Ok(Json(json!({ "ok": true })))
}

fn safe_theme(theme: &ThemeConfig) -> bool {
    super::super::theme_routes::valid_theme_id(&theme.name)
        && [
            &theme.background,
            &theme.foreground,
            &theme.cursor,
            &theme.selection,
            &theme.black,
            &theme.red,
            &theme.green,
            &theme.yellow,
            &theme.blue,
            &theme.magenta,
            &theme.cyan,
            &theme.white,
            &theme.bright_black,
            &theme.bright_red,
            &theme.bright_green,
            &theme.bright_yellow,
            &theme.bright_blue,
            &theme.bright_magenta,
            &theme.bright_cyan,
            &theme.bright_white,
        ]
        .into_iter()
        .all(|value| {
            value.strip_prefix('#').is_some_and(|hex| {
                hex.len() == 6 && hex.bytes().all(|byte| byte.is_ascii_hexdigit())
            })
        })
}
