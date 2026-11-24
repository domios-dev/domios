use codeterm_service_core::config::schema::get_any_theme;
use codeterm_types::svg::is_current_color_stroke_tile;

use super::*;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Selection {
    Settings {
        name: String,
    },
    Preset {
        name: String,
        expected: Box<AppearanceLook>,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PresetPreview {
    name: String,
    expected: AppearanceLook,
}

#[derive(Serialize)]
pub(super) struct SwitchReceipt {
    #[serde(flatten)]
    receipt: Receipt,
    theme_label: String,
    wallpaper_switched: bool,
}

fn owner_target(state: &AppState, headers: &HeaderMap, machine: &str) -> Result<Machine, Refusal> {
    if crate::auth::authenticated_origin(state, headers)? != AuthenticatedOrigin::Owner {
        return Err((StatusCode::FORBIDDEN, "Owner authorization required".into()));
    }
    target(state, headers, machine)
}

fn palette_preset(name: &str) -> Result<ThemeConfig, Refusal> {
    presets::all()
        .into_iter()
        .find(|theme| theme.name == name)
        .ok_or((StatusCode::NOT_FOUND, "Preset is unavailable".into()))
}

pub(super) async fn preview(
    State(state): State<AppState>,
    Path(machine): Path<String>,
    headers: HeaderMap,
    Extension(assets): Extension<Arc<WallpaperAssets>>,
    Json(draft): Json<PresetPreview>,
) -> Result<Json<AppearanceLook>, ApiRefusal> {
    crate::auth::off_runtime(&state, &headers, move |state, headers| {
        owner_target(state, headers, &machine)?;
        let theme = palette_preset(&draft.name)?;
        let _writer = assets.settings_writer.lock();
        let current = AppearanceLook::from_config(&state.config.lock());
        require_current(&draft.expected, &current)?;
        Ok(Json(theme_look(&current, theme, |id| assets.svg_asset(id))))
    })
    .await
    .map_err(typed_refusal)
}

pub(super) async fn apply(
    State(state): State<AppState>,
    Path(machine): Path<String>,
    headers: HeaderMap,
    Extension(assets): Extension<Arc<WallpaperAssets>>,
    Json(selection): Json<Selection>,
) -> Result<Json<Option<SwitchReceipt>>, ApiRefusal> {
    crate::auth::off_runtime(&state, &headers, move |state, headers| {
        let machine = owner_target(state, headers, &machine)?;
        let (theme, label, expected) = match selection {
            Selection::Settings { name } => {
                if !crate::theme_routes::valid_theme_id(&name) {
                    return Err(bad_request("Invalid theme identity"));
                }
                let mut theme = get_any_theme(&name)
                    .ok_or((StatusCode::NOT_FOUND, "Theme is unavailable".into()))?;
                let label = std::mem::replace(&mut theme.name, name);
                (theme, label, None)
            }
            Selection::Preset { name, expected } => (palette_preset(&name)?, name, Some(expected)),
        };
        let _writer = assets.settings_writer.lock();
        let id = uuid::Uuid::new_v4().simple().to_string();
        let mut refused = None;
        let config = update_appearance(state, &assets, |config| {
            commit(config, &assets, &id, theme, expected.as_deref()).map_err(|error| {
                let message = error.1.clone();
                refused = Some(error);
                message
            })
        })
        .map_err(|error| refused.unwrap_or((StatusCode::INTERNAL_SERVER_ERROR, error)))?;
        let Ok(proposal) = find(&config.appearance_proposals, &id) else {
            return Ok(Json(None));
        };
        publish_change(state, &config, &id, false);
        Ok(Json(Some(SwitchReceipt {
            wallpaper_switched: proposal.look.wallpaper != proposal.previous.wallpaper,
            theme_label: label,
            receipt: receipt_with_checks(machine, proposal.clone(), &config, Vec::new()),
        })))
    })
    .await
    .map_err(typed_refusal)
}

fn require_current(expected: &AppearanceLook, current: &AppearanceLook) -> Result<(), Refusal> {
    if expected == current {
        Ok(())
    } else {
        Err((
            StatusCode::CONFLICT,
            "Appearance changed. Reload the Lab before applying.".into(),
        ))
    }
}

fn commit(
    config: &mut AppConfig,
    assets: &WallpaperAssets,
    id: &str,
    theme: ThemeConfig,
    expected: Option<&AppearanceLook>,
) -> Result<(), Refusal> {
    let previous = AppearanceLook::from_config(config);
    if let Some(expected) = expected {
        require_current(expected, &previous)?;
    }
    let look = theme_look(&previous, theme, |id| assets.svg_asset(id));
    if look == previous {
        return Ok(());
    }
    let mut proposal = AppearanceProposal {
        id: id.into(),
        rationale: "Selected a theme".into(),
        tweak: false,
        look,
        previous,
        previous_active: config.appearance_active.clone(),
        state: AppearanceState::Proposed,
        applied_at: None,
    };
    apply_look(config, &mut proposal, ContrastPolicy::NamedTheme)?;
    retain_proposal(config, proposal);
    Ok(())
}

fn theme_look(
    current: &AppearanceLook,
    theme: ThemeConfig,
    read_svg: impl FnOnce(&str) -> Option<Vec<u8>>,
) -> AppearanceLook {
    let wallpaper = current
        .wallpaper
        .as_ref()
        .filter(|paper| {
            contrast::theme_is_light(&current.theme) == contrast::theme_is_light(&theme)
                || recolorable(paper, read_svg)
        })
        .cloned();
    AppearanceLook {
        theme,
        wallpaper,
        tuning: current.tuning,
    }
}

fn recolorable(paper: &ChatWallpaper, read_svg: impl FnOnce(&str) -> Option<Vec<u8>>) -> bool {
    let background = &paper.background;
    if background.gradient.is_some()
        || background.color.is_some()
        || background.colors.is_some()
        || (paper.fill.is_some() && paper.fill_strength.is_some())
    {
        return false;
    }
    match background.pattern {
        ChatPattern::Image => false,
        ChatPattern::Svg => match paper.asset.as_deref() {
            Some(id) => read_svg(id).is_some_and(|svg| is_current_color_stroke_tile(&svg)),
            None => background
                .svg
                .as_ref()
                .is_some_and(|svg| is_current_color_stroke_tile(svg.as_bytes())),
        },
        _ => true,
    }
}

#[cfg(test)]
mod tests;
