use super::{
    activation::Activation,
    manifest::{newer_than, Manifest},
};
use codeterm_plugins::{AvailabilityItem, AvailabilityState, UpdateKind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rendered {
    pub version: String,
    pub release_id: Option<String>,
}

pub struct Offer<'a> {
    pub version: &'a str,
    pub compatible: bool,
}

// The app embeds a frontend of its own version, so a stored release survives startup only while it is newer.
pub fn settle(state: &mut Activation, app: &semver::Version, api_version: u32) {
    state.recover(api_version);
    state.retire_not_newer(app);
}

pub fn rendered(state: &Activation, app: &semver::Version) -> Rendered {
    match &state.active {
        Some(release) => Rendered {
            version: release.version.clone(),
            release_id: Some(release.release_id.clone()),
        },
        None => Rendered {
            version: app.to_string(),
            release_id: None,
        },
    }
}

pub fn staged(state: &Activation) -> Option<&Manifest> {
    state
        .pending
        .as_ref()
        .filter(|m| state.loading.is_none() && !state.quarantined.contains(&m.release_id))
}

pub fn availability(
    installed: &str,
    staged: Option<&Manifest>,
    channel: Option<Offer<'_>>,
    api_version: u32,
) -> AvailabilityItem {
    if let Some(release) =
        staged.filter(|m| m.compatible(api_version) && ahead(installed, &m.version))
    {
        let mut item = release_item(installed, &release.version, true);
        item.state = AvailabilityState::ReadyToApply;
        item.reason = "staged_bundle".into();
        return item;
    }
    offer_availability(installed, channel)
}

pub fn offer_availability(installed: &str, channel: Option<Offer<'_>>) -> AvailabilityItem {
    let Some(offer) = channel else {
        return release_item(installed, installed, true);
    };
    let decision = release_item(installed, offer.version, offer.compatible);
    if offer.compatible && ahead(installed, offer.version) {
        return decision;
    }
    let mut item = release_item(installed, installed, true);
    item.reason = decision.reason;
    item
}

fn ahead(installed: &str, version: &str) -> bool {
    semver::Version::parse(installed).is_ok_and(|floor| newer_than(version, &floor))
}

fn release_item(installed: &str, latest: &str, compatible: bool) -> AvailabilityItem {
    AvailabilityItem::release(
        "frontend",
        UpdateKind::Frontend,
        installed,
        latest,
        compatible,
    )
}

#[cfg(test)]
mod tests;
