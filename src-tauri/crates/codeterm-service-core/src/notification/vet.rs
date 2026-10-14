//! Caller-side vetting shared by every notification.create transport.

use codeterm_notification_core::contract::{validate_answer_labels, CallerAction};
use serde::Deserialize;

use crate::notification::reach::Unseen;

#[derive(Deserialize)]
pub struct CreateNotifBody {
    pub(crate) title: String,
    pub(crate) body: Option<String>,
    pub(crate) severity: Option<String>,
    /// Typed at the boundary as the CALLER-facing action set, which has no
    /// invoke variant: a command name has nowhere to land, so it is not a
    /// value some later click handler could interpret.
    pub(crate) actions: Option<Vec<CallerAction>>,
    pub(crate) sticky: Option<bool>,
    pub(crate) tab_id: Option<String>,
    /// Stamped by the Mesh relay from the signed sender; suppresses any tab claim.
    pub(crate) source_peer: Option<String>,
    pub(crate) level: Option<String>,
    pub(crate) event_key: Option<String>,
    /// The surface this notification offers to turn on: `desktop`, or the
    /// service id of a registered push channel. A caller names a surface, never
    /// a config key, and the change still waits for the tap.
    pub(crate) offer: Option<String>,
    /// Which offered label applies the offer. Must be one of this
    /// notification's own choices.
    pub(crate) offer_accept: Option<String>,
    /// The tab whose chat holds the message this notification jumps to.
    pub(crate) target_tab_id: Option<String>,
    /// The exact chat message id inside `target_tab_id`.
    pub(crate) chat_message_id: Option<String>,
}

/// Adopt a caller's offer to be louder. The surface is resolved against the
/// registry and the accepting label against this notification's own choice
/// set, so an offer can neither name a destination that resolves to nothing
/// nor apply on a label no surface will draw.
/// A tap opens the offered column on the row of THIS notification's level, so
/// an offer that names no level could only ever apply nothing. Refused when the
/// offer is made rather than accepted and left silently inert, because an offer
/// that can never apply is a button that lies.
pub(crate) fn offer_requires_level(
    named: &str,
    level: Option<crate::notification::push::NotifyLevel>,
) -> Result<crate::notification::push::NotifyLevel, String> {
    level.ok_or_else(|| {
        format!(
            "Offering '{named}' needs a level: the tap opens that column for the level this notification carries, and one that names no level has no row to open"
        )
    })
}

pub fn vet_offer(
    svc: &crate::service::ServiceState,
    offer: Option<String>,
    offer_accept: Option<String>,
    actions: Option<&[CallerAction]>,
    level: Option<crate::notification::push::NotifyLevel>,
) -> Result<Option<crate::notification::authority::SurfaceOffer>, String> {
    let Some(named) = offer.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        if offer_accept.is_some() {
            return Err("An accepting label needs a surface to offer".to_string());
        }
        return Ok(None);
    };
    let accept = offer_accept
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "An offered surface needs the label that accepts it".to_string())?;
    offer_requires_level(named, level)?;
    let target = svc.resolve_surface_target(named)?;
    crate::notification::authority::adopt_offer(target, accept, actions.unwrap_or_default())
        .map(Some)
}

/// An unknown level is rejected here rather than degrading to a runtime
/// string, so an invented level can never be expressed on any transport.
pub fn parse_push_intent(
    level: Option<&str>,
    event_key: Option<String>,
) -> Result<crate::service::notification::PushIntent, String> {
    let level = match level.map(str::trim).filter(|c| !c.is_empty()) {
        None => None,
        Some(raw) => Some(
            crate::notification::push::NotifyLevel::parse(raw).ok_or_else(|| {
                format!(
                    "Unknown notification level: expected one of {}",
                    crate::notification::push::NotifyLevel::wire_values().join(", ")
                )
            })?,
        ),
    };
    Ok(crate::service::notification::PushIntent { level, event_key })
}

/// A caller's title, trimmed and bounded by the one rule every transport shares.
pub fn vet_title(title: &str) -> Result<String, String> {
    codeterm_notification_core::contract::validate_title(title).map(str::to_string)
}

/// A jump target must be a tab on this daemon, or the record would open nothing.
pub fn vet_target_tab(
    target_tab_id: Option<&str>,
    tab_exists: impl FnOnce(&str) -> bool,
) -> Result<(), String> {
    let Some(target) = target_tab_id else {
        return Ok(());
    };
    let target = target.trim();
    if target.is_empty() || !tab_exists(target) {
        return Err(format!("Target tab '{target}' does not exist"));
    }
    Ok(())
}

/// Vet a caller-supplied action set. Parsing already closed the kind - and the
/// type it parsed into has no invoke variant at all; this closes the choice
/// set, which only makes sense as a whole.
pub fn vet_actions(
    actions: Option<Vec<CallerAction>>,
) -> Result<Option<Vec<CallerAction>>, String> {
    if let Some(ref actions) = actions {
        validate_answer_labels(actions)?;
    }
    Ok(actions)
}

/// The one shape `notification.create` answers with, on every transport. The
/// id is unconditional — the record was opened — and `unseen` appears only when
/// no surface will show it, so its presence is itself the signal.
pub fn created_result(id: &str, unseen: Option<Unseen>) -> serde_json::Value {
    let mut result = serde_json::json!({ "id": id });
    if let Some(unseen) = unseen {
        result["unseen"] = serde_json::to_value(unseen).unwrap_or_default();
    }
    result
}
