use std::hash::{DefaultHasher, Hash, Hasher};

use axum::extract::rejection::JsonRejection;
use codeterm_service_core::service::general_agent::session_contract::owns_main;
use serde_json::json;

use super::*;

pub(super) const MAX_TWEAK_BYTES: usize = 4096;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Draft {
    revision: String,
    toml: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Patch {
    appearance: Option<Tuning>,
    chat_wallpaper: Option<Wallpaper>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Tuning {
    roundness: Option<f64>,
    shadow_strength: Option<f64>,
    border_strength: Option<f64>,
    font_size: Option<i8>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Wallpaper {
    background: Option<Background>,
    fill_strength: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Background {
    opacity: f64,
}

impl Patch {
    fn update(self, look: &mut AppearanceLook) -> Result<(), Refusal> {
        if let Some(tuning) = self.appearance {
            let mut next = look.tuning.unwrap_or_default();
            let previous = next;
            next.roundness = tuning.roundness.unwrap_or(next.roundness);
            next.shadow_strength = tuning.shadow_strength.unwrap_or(next.shadow_strength);
            next.border_strength = tuning.border_strength.unwrap_or(next.border_strength);
            next.font_size = tuning.font_size.unwrap_or(next.font_size);
            if next != previous {
                look.tuning = Some(next);
            }
        }
        if let Some(wallpaper) = self.chat_wallpaper {
            let paper = look.wallpaper.as_mut().ok_or_else(|| {
                proposal_required("Wallpaper controls require a machine wallpaper.")
            })?;
            if let Some(background) = wallpaper.background {
                if !(0.0..=1.0).contains(&background.opacity) {
                    return Err(bad_request("Wallpaper intensity must be between 0 and 1"));
                }
                paper.background.opacity = Some(background.opacity);
            }
            if let Some(strength) = wallpaper.fill_strength {
                if strength != paper.fill_strength.unwrap_or(1.0) {
                    paper.fill_strength = Some(strength);
                }
            }
        }
        Ok(())
    }
}

fn proposal_required(detail: &str) -> Refusal {
    (
        StatusCode::BAD_REQUEST,
        json!({
            "code": "appearance_proposal_required",
            "message": format!("{detail} Use domios appearance propose and return its native <ds-appearance> card for review."),
        })
        .to_string(),
    )
}

pub(super) fn revision(look: &AppearanceLook, active: Option<&str>) -> Result<String, Refusal> {
    let snapshot = serde_json::to_vec(&(look, active))
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let mut hasher = DefaultHasher::new();
    snapshot.hash(&mut hasher);
    Ok(format!("{:016x}", hasher.finish()))
}

fn require_bound_main(state: &AppState, headers: &HeaderMap) -> Result<(), Refusal> {
    if let CallerIdentity::Tab(caller) = crate::auth::caller_identity(state, headers)? {
        let mux = state.svc.mux.read();
        if mux.get_tab(&caller.tab_id).is_some_and(|tab| {
            tab.protected
                && owns_main(&mux, tab)
                && tab.neighbor_authority.is_none()
                && tab.launch_session_authenticated()
                && tab.session_binding_nonce.as_deref() == Some(caller.binding_nonce.as_str())
        }) {
            return Ok(());
        }
    }
    Err((
        StatusCode::FORBIDDEN,
        json!({
            "code": "appearance_main_required",
            "message": "Only the machine's authenticated General Main session can apply a tweak.",
        })
        .to_string(),
    ))
}

pub(super) async fn apply(
    State(state): State<AppState>,
    Path(machine): Path<String>,
    headers: HeaderMap,
    Extension(assets): Extension<Arc<WallpaperAssets>>,
    draft: Result<Json<Draft>, JsonRejection>,
) -> Result<Json<Receipt>, ApiRefusal> {
    crate::auth::off_runtime(&state, &headers, move |state, headers| {
        let _writer = assets.settings_writer.lock();
        require_bound_main(state, headers)?;
        let machine = local_machine(state, &machine)?;
        let Json(draft) = draft.map_err(|error| proposal_required(&error.body_text()))?;
        if draft.toml.len() > MAX_TWEAK_BYTES {
            return Err(bad_request("Appearance tweaks must fit in 4 KiB"));
        }
        let patch: Patch =
            toml::from_str(&draft.toml).map_err(|error| proposal_required(error.message()))?;
        let id = uuid::Uuid::new_v4().simple().to_string();
        let mut refusal = None;
        let config = update_appearance(state, &assets, |config| {
            apply_patch(config, &id, &draft.revision, patch).map_err(|error| {
                let message = error.1.clone();
                refusal = Some(error);
                message
            })
        })
        .map_err(|error| refusal.unwrap_or((StatusCode::INTERNAL_SERVER_ERROR, error)))?;
        let proposal = find(&config.appearance_proposals, &id)?.clone();
        publish_change(state, &config, &id, false);
        receipt(machine, proposal, &config).map(Json)
    })
    .await
    .map_err(typed_refusal)
}

fn apply_patch(
    config: &mut AppConfig,
    id: &str,
    expected_revision: &str,
    patch: Patch,
) -> Result<(), Refusal> {
    let previous = AppearanceLook::from_config(config);
    if revision(&previous, config.appearance_active.as_deref())? != expected_revision {
        return Err((
            StatusCode::CONFLICT,
            json!({
                "code": "appearance_changed",
                "message": "Appearance changed. Read domios appearance current before tweaking it.",
            })
            .to_string(),
        ));
    }
    let mut look = previous.clone();
    patch.update(&mut look)?;
    if look == previous {
        return Err(bad_request("The tweak would leave every control unchanged"));
    }
    let mut proposal = AppearanceProposal {
        id: id.to_owned(),
        rationale: String::new(),
        tweak: true,
        look,
        previous,
        previous_active: config.appearance_active.clone(),
        state: AppearanceState::Proposed,
        applied_at: None,
    };
    apply_proposal(config, &mut proposal)?;
    retain_proposal(config, proposal);
    Ok(())
}
