use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Extension, Json, Router,
};
use codeterm_neighbors_service::NeighborsNeighborsRuntime1Ext;
use codeterm_service_core::config::schema::{
    AppConfig, AppearanceLook, AppearanceProposal, AppearanceState, ChatPattern, ChatWallpaper,
    ThemeConfig,
};
use serde::{Deserialize, Serialize};

use crate::{
    auth::{AuthenticatedOrigin, CallerIdentity},
    routes::AppState,
    wallpaper_routes::{update_appearance, validate_wallpaper, WallpaperAssets},
};

mod contrast;
mod presets;
mod theme_switch;
mod tweak;
use contrast::{measure, ContrastCheck};

const PROPOSAL_LIMIT: usize = 12;
type Refusal = (StatusCode, String);
pub(super) type ApiRefusal = (StatusCode, Json<serde_json::Value>);

pub(super) fn typed_refusal((status, message): Refusal) -> ApiRefusal {
    let data = serde_json::from_str(&message)
        .unwrap_or_else(|_| serde_json::json!({ "message": message }));
    (status, Json(data))
}

pub(crate) fn router() -> Router<AppState> {
    Router::new()
        .route("/api/appearance/{machine}", get(read).post(propose))
        .route("/api/appearance/{machine}/validate", post(diagnose))
        .route("/api/appearance/{machine}/tweak", post(tweak::apply))
        .route(
            "/api/appearance/{machine}/switch-theme",
            post(theme_switch::apply),
        )
        .route(
            "/api/appearance/{machine}/preview-preset",
            post(theme_switch::preview),
        )
        .route("/api/appearance/{machine}/{id}", get(show))
        .route("/api/appearance/{machine}/{id}/apply", post(apply))
        .route("/api/appearance/{machine}/{id}/undo", post(undo))
        .route(
            "/api/appearance/{machine}/{id}/use-theme-wallpaper",
            post(use_theme_wallpaper),
        )
}

fn target(state: &AppState, headers: &HeaderMap, selection: &str) -> Result<Machine, Refusal> {
    let origin = crate::auth::authenticated_origin(state, headers)?;
    let identity = crate::auth::caller_identity(state, headers)?;
    let authorized = match identity {
        CallerIdentity::Local => origin == AuthenticatedOrigin::Owner,
        CallerIdentity::Peer { access_level, .. } => {
            origin == AuthenticatedOrigin::Owner
                && access_level == codeterm_service_core::mesh::types::AccessLevel::Full
        }
        CallerIdentity::Tab(caller) => state
            .svc
            .require_neighbor_general(Some(&caller.tab_id), Some(&caller.binding_nonce))
            .is_ok(),
        _ => false,
    };
    if !authorized {
        return Err((StatusCode::FORBIDDEN, "Owner authorization required".into()));
    }
    local_machine(state, selection)
}

fn local_machine(state: &AppState, selection: &str) -> Result<Machine, Refusal> {
    let mesh = state.mesh.lock();
    let id = &mesh.ident.identity.instance_id;
    let name = &mesh.ident.instance_name;
    if selection != "local" && selection != id && selection != name {
        return Err((
            StatusCode::NOT_FOUND,
            "Appearance machine does not match this host".into(),
        ));
    }
    Ok(Machine {
        id: id.clone(),
        name: name.clone(),
    })
}

#[derive(Clone, Serialize)]
pub(crate) struct Machine {
    id: String,
    name: String,
}

#[derive(Serialize)]
struct Current {
    machine: Machine,
    look: AppearanceLook,
    revision: String,
    presets: Vec<ThemeConfig>,
    active: Option<Receipt>,
}

#[derive(Serialize)]
struct Receipt {
    machine: Machine,
    #[serde(flatten)]
    proposal: AppearanceProposal,
    contrast: Vec<ContrastCheck>,
    can_undo: bool,
}

fn receipt(
    machine: Machine,
    proposal: AppearanceProposal,
    config: &codeterm_service_core::config::schema::AppConfig,
) -> Result<Receipt, Refusal> {
    let contrast = measure(&proposal.look).map_err(bad_request)?;
    Ok(receipt_with_checks(machine, proposal, config, contrast))
}

fn receipt_with_checks(
    machine: Machine,
    proposal: AppearanceProposal,
    config: &AppConfig,
    contrast: Vec<ContrastCheck>,
) -> Receipt {
    let can_undo = proposal.state == AppearanceState::Applied
        && config.appearance_active.as_deref() == Some(proposal.id.as_str())
        && proposal.look == AppearanceLook::from_config(config);
    Receipt {
        machine,
        proposal,
        contrast,
        can_undo,
    }
}

async fn read(
    State(state): State<AppState>,
    Path(machine): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Current>, Refusal> {
    crate::auth::off_runtime(&state, &headers, move |state, headers| {
        let machine = target(state, headers, &machine)?;
        let config = state.config.lock().clone();
        let look = AppearanceLook::from_config(&config);
        let active = config
            .appearance_active
            .as_ref()
            .and_then(|id| find(&config.appearance_proposals, id).ok())
            .map(|record| receipt(machine.clone(), record.clone(), &config))
            .transpose()?
            .filter(|record| record.can_undo);
        Ok(Json(Current {
            machine,
            revision: tweak::revision(&look, config.appearance_active.as_deref())?,
            look,
            presets: presets::all(),
            active,
        }))
    })
    .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Draft {
    rationale: String,
    look: Option<AppearanceLook>,
    toml: Option<String>,
    expected: Option<AppearanceLook>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LookFile {
    theme: ThemeConfig,
    appearance: Option<codeterm_service_core::config::schema::AppearanceTuning>,
    chat_wallpaper: Option<ChatWallpaper>,
}

async fn diagnose(
    State(state): State<AppState>,
    Path(machine): Path<String>,
    headers: HeaderMap,
    Json(look): Json<AppearanceLook>,
) -> Result<Json<Vec<ContrastCheck>>, Refusal> {
    crate::auth::off_runtime(&state, &headers, move |state, headers| {
        target(state, headers, &machine)?;
        measure(&look).map(Json).map_err(bad_request)
    })
    .await
}

async fn propose(
    State(state): State<AppState>,
    Path(machine): Path<String>,
    headers: HeaderMap,
    Extension(assets): Extension<Arc<WallpaperAssets>>,
    Json(draft): Json<Draft>,
) -> Result<Json<Receipt>, ApiRefusal> {
    crate::auth::off_runtime(&state, &headers, move |state, headers| {
        let machine = target(state, headers, &machine)?;
        let rationale = draft.rationale.trim().to_owned();
        if rationale.is_empty() || rationale.len() > 480 || rationale.chars().any(char::is_control)
        {
            return Err(bad_request(
                "Provide a single-line rationale of up to 480 bytes",
            ));
        }
        let mut look = match (draft.look, draft.toml) {
            (Some(look), None) => look,
            (None, Some(toml)) if toml.len() <= 256 * 1024 => {
                let file: LookFile = toml::from_str(&toml).map_err(|error| {
                    bad_request(format!("Invalid appearance TOML: {}", error.message()))
                })?;
                AppearanceLook {
                    theme: file.theme,
                    tuning: file.appearance,
                    wallpaper: file.chat_wallpaper,
                }
            }
            _ => {
                return Err(bad_request(
                    "Provide exactly one look or TOML file (up to 256 KiB)",
                ))
            }
        };
        let _writer = assets.settings_writer.lock();
        validate_wallpaper(&assets, &mut look.wallpaper)
            .map_err(|(status, reason)| (status, reason.to_owned()))?;
        validate(&look)?;
        look.tuning = Some(look.tuning.unwrap_or_default());
        let id = uuid::Uuid::new_v4().simple().to_string();
        let mut refused = None;
        let config = update_appearance(state, &assets, |config| {
            let previous = AppearanceLook::from_config(config);
            if draft
                .expected
                .as_ref()
                .is_some_and(|expected| *expected != previous)
            {
                return reject(
                    &mut refused,
                    StatusCode::CONFLICT,
                    "Appearance changed. Reload the Lab before applying.",
                );
            }
            let proposal = AppearanceProposal {
                id: id.clone(),
                rationale,
                tweak: false,
                look,
                previous,
                previous_active: config.appearance_active.clone(),
                state: AppearanceState::Proposed,
                applied_at: None,
            };
            retain_proposal(config, proposal);
            Ok(())
        })
        .map_err(|error| refused.unwrap_or((StatusCode::INTERNAL_SERVER_ERROR, error)))?;
        let proposal = find(&config.appearance_proposals, &id)?.clone();
        receipt(machine, proposal, &config).map(Json)
    })
    .await
    .map_err(typed_refusal)
}

fn retain_proposal(config: &mut AppConfig, proposal: AppearanceProposal) {
    let history = &mut config.appearance_proposals;
    if history.len() >= PROPOSAL_LIMIT {
        let oldest = history
            .iter()
            .position(|item| proposal.previous_active.as_ref() != Some(&item.id))
            .unwrap_or(0);
        history.remove(oldest);
    }
    history.push(proposal);
}

fn apply_proposal(
    config: &mut AppConfig,
    proposal: &mut AppearanceProposal,
) -> Result<(), Refusal> {
    apply_look(config, proposal, ContrastPolicy::Enforce)
}

#[derive(Clone, Copy)]
enum ContrastPolicy {
    Enforce,
    NamedTheme,
}

fn apply_look(
    config: &mut AppConfig,
    proposal: &mut AppearanceProposal,
    policy: ContrastPolicy,
) -> Result<(), Refusal> {
    validate_structure(&proposal.look)?;
    match policy {
        ContrastPolicy::Enforce => enforce_contrast(&proposal.look)?,
        ContrastPolicy::NamedTheme => {
            contrast::validate_palette(&proposal.look.theme).map_err(bad_request)?;
        }
    }
    proposal.look.write_to(config);
    config.appearance_active = Some(proposal.id.clone());
    proposal.applied_at = Some(unix_seconds());
    proposal.state = AppearanceState::Applied;
    Ok(())
}

async fn show(
    State(state): State<AppState>,
    Path((machine, id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<Receipt>, ApiRefusal> {
    crate::auth::off_runtime(&state, &headers, move |state, headers| {
        let machine = target(state, headers, &machine)?;
        let config = state.config.lock().clone();
        let proposal = find(&config.appearance_proposals, &id)?.clone();
        receipt(machine, proposal, &config).map(Json)
    })
    .await
    .map_err(typed_refusal)
}

async fn apply(
    State(state): State<AppState>,
    Path((machine, id)): Path<(String, String)>,
    headers: HeaderMap,
    Extension(assets): Extension<Arc<WallpaperAssets>>,
) -> Result<Json<Receipt>, ApiRefusal> {
    transition(
        state,
        &headers,
        assets,
        machine,
        id,
        AppearanceState::Applied,
        false,
    )
    .await
    .map_err(typed_refusal)
}

async fn undo(
    State(state): State<AppState>,
    Path((machine, id)): Path<(String, String)>,
    headers: HeaderMap,
    Extension(assets): Extension<Arc<WallpaperAssets>>,
) -> Result<Json<Receipt>, ApiRefusal> {
    transition(
        state,
        &headers,
        assets,
        machine,
        id,
        AppearanceState::Undone,
        false,
    )
    .await
    .map_err(typed_refusal)
}

async fn use_theme_wallpaper(
    State(state): State<AppState>,
    Path((machine, id)): Path<(String, String)>,
    headers: HeaderMap,
    Extension(assets): Extension<Arc<WallpaperAssets>>,
) -> Result<Json<Receipt>, ApiRefusal> {
    transition(
        state,
        &headers,
        assets,
        machine,
        id,
        AppearanceState::Applied,
        true,
    )
    .await
    .map_err(typed_refusal)
}

async fn transition(
    state: AppState,
    headers: &HeaderMap,
    assets: Arc<WallpaperAssets>,
    machine: String,
    id: String,
    next: AppearanceState,
    use_theme_wallpaper: bool,
) -> Result<Json<Receipt>, Refusal> {
    crate::auth::off_runtime(&state, headers, move |state, headers| {
        let machine = target(state, headers, &machine)?;
        let owner = crate::auth::authenticated_origin(state, headers)? == AuthenticatedOrigin::Owner;
        if !owner && next != AppearanceState::Undone {
            return Err((StatusCode::FORBIDDEN, "Owner authorization required".into()));
        }
        let _writer = assets.settings_writer.lock();
        let mut refused = None;
        let mut changed = false;
        let config = update_appearance(state, &assets, |config| {
            let mut proposal = find(&config.appearance_proposals, &id).cloned().map_err(|(status, message)| {
                refused = Some((status, message.clone()));
                message
            })?;
            if !owner && !proposal.tweak {
                return reject(&mut refused, StatusCode::FORBIDDEN, "Owner authorization required");
            }
            let current = AppearanceLook::from_config(config);
            let is_current = current == proposal.look && config.appearance_active.as_deref() == Some(id.as_str());
            if use_theme_wallpaper && proposal.tweak {
                return reject(&mut refused, StatusCode::CONFLICT, "Use a proposal card to change the wallpaper.");
            }
            match (proposal.state, next) {
                (AppearanceState::Applied, AppearanceState::Undone) if is_current => {
                    proposal.previous.write_to(config);
                    config.appearance_active.clone_from(&proposal.previous_active);
                }
                (AppearanceState::Proposed | AppearanceState::Undone, AppearanceState::Applied)
                    if current == proposal.previous && config.appearance_active == proposal.previous_active => {}
                (AppearanceState::Applied, AppearanceState::Applied)
                    if use_theme_wallpaper && is_current && proposal.look.wallpaper.is_some() => {}
                (state, requested) if state == requested && (!use_theme_wallpaper || is_current) => return Ok(()),
                _ => return reject(&mut refused, StatusCode::CONFLICT, "Appearance changed after this proposal. Open the Lab to review the current look."),
            }
            if next == AppearanceState::Applied {
                if use_theme_wallpaper {
                    let has_theme_wallpaper = proposal.look.theme.chat_background.as_ref().is_some_and(|background| {
                        background.pattern != ChatPattern::None || background.gradient.is_some()
                    });
                    if !has_theme_wallpaper || proposal.look.wallpaper.take().is_none() {
                        return reject(&mut refused, StatusCode::CONFLICT, "This proposal has no theme wallpaper hidden by a machine override.");
                    }
                }
                apply_proposal(config, &mut proposal).map_err(|(status, message)| {
                    refused = Some((status, message.clone()));
                    message
                })?;
            }
            proposal.state = next;
            changed = true;
            if let Some(stored) = config.appearance_proposals.iter_mut().find(|stored| stored.id == id) {
                *stored = proposal;
            }
            Ok(())
        }).map_err(|error| refused.unwrap_or((StatusCode::INTERNAL_SERVER_ERROR, error)))?;
        let proposal = find(&config.appearance_proposals, &id)?.clone();
        if changed {
            publish_change(state, &config, &id, next == AppearanceState::Undone);
        }
        receipt(machine, proposal, &config).map(Json)
    }).await
}

fn publish_change(state: &AppState, config: &AppConfig, id: &str, undo: bool) {
    emit_change(state, config, Some(id), undo);
    crate::wallpaper_routes::publish_config(state, config);
}

pub(crate) fn emit_change(
    state: &AppState,
    config: &codeterm_service_core::config::schema::AppConfig,
    proposal: Option<&str>,
    undo: bool,
) {
    let machine = state.mesh.lock().ident.identity.instance_id.clone();
    state.svc.ui.emit(
        "appearance-committed",
        serde_json::json!({
            "event": uuid::Uuid::new_v4().simple().to_string(),
            "machine": machine,
            "proposal": proposal,
            "undo": undo,
            "look": AppearanceLook::from_config(config),
        }),
    );
}

pub(crate) fn validate(look: &AppearanceLook) -> Result<(), Refusal> {
    validate_structure(look)?;
    enforce_contrast(look)
}

fn validate_structure(look: &AppearanceLook) -> Result<(), Refusal> {
    let name = look.theme.name.trim();
    if name.is_empty()
        || name.len() > 100
        || name
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\'))
    {
        return Err(bad_request(
            "Provide a single-line look name of up to 100 bytes",
        ));
    }
    look.tuning
        .unwrap_or_default()
        .validate()
        .map_err(bad_request)
}

fn enforce_contrast(look: &AppearanceLook) -> Result<(), Refusal> {
    let checks = measure(look).map_err(bad_request)?;
    let failures: Vec<_> = checks
        .iter()
        .filter(|check| check.ratio < check.floor)
        .collect();
    if failures.is_empty() {
        Ok(())
    } else {
        Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            serde_json::json!({
                "code": "appearance_contrast",
                "message": "This look falls below the required contrast",
                "failures": failures,
            })
            .to_string(),
        ))
    }
}

fn find<'a>(
    history: &'a [AppearanceProposal],
    id: &str,
) -> Result<&'a AppearanceProposal, Refusal> {
    history.iter().find(|record| record.id == id).ok_or((
        StatusCode::NOT_FOUND,
        "Appearance proposal not found".into(),
    ))
}

fn bad_request(message: impl Into<String>) -> Refusal {
    (StatusCode::BAD_REQUEST, message.into())
}

fn reject(refusal: &mut Option<Refusal>, status: StatusCode, message: &str) -> Result<(), String> {
    *refusal = Some((status, message.into()));
    Err(message.into())
}

fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
