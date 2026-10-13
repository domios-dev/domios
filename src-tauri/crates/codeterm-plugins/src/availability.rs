use serde::Serialize;

pub enum UpdateSource<'a> {
    Local,
    Channel(&'a str),
    Unknown,
}

pub struct UpdateOffer<'a> {
    pub channel: &'a str,
    pub version: Option<&'a str>,
    pub payload_version: Option<&'a str>,
    pub compatible: bool,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct UpdateAvailability {
    pub available: bool,
    pub reason: &'static str,
}

pub fn update_availability(
    installed_version: Option<&str>,
    source: UpdateSource<'_>,
    offer: Option<UpdateOffer<'_>>,
) -> UpdateAvailability {
    let reason = match source {
        UpdateSource::Local => "local_install",
        UpdateSource::Unknown => "unknown_source",
        UpdateSource::Channel(channel) => match offer {
            None => "channel_unavailable",
            Some(offer) if offer.channel != channel => "different_channel",
            Some(offer) => version_decision(installed_version, offer),
        },
    };
    UpdateAvailability {
        available: reason == "newer_version",
        reason,
    }
}

fn version_decision(installed: Option<&str>, offer: UpdateOffer<'_>) -> &'static str {
    let (Some(installed), Some(remote), Some(payload)) =
        (installed, offer.version, offer.payload_version)
    else {
        return "missing_version";
    };
    let (Ok(installed), Ok(remote), Ok(payload)) = (
        semver::Version::parse(installed),
        semver::Version::parse(remote),
        semver::Version::parse(payload),
    ) else {
        return "invalid_version";
    };
    if remote != payload {
        return "channel_manifest_mismatch";
    }
    if !offer.compatible {
        return "incompatible";
    }
    match remote.cmp_precedence(&installed) {
        std::cmp::Ordering::Greater => "newer_version",
        std::cmp::Ordering::Equal => "same_version",
        std::cmp::Ordering::Less => "local_ahead",
    }
}

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AvailabilityState {
    UpToDate,
    UpdateAvailable,
    Incompatible,
    CoreSilent,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpdateKind {
    Plugin,
    App,
    Frontend,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct AvailabilityItem {
    pub id: String,
    pub kind: UpdateKind,
    pub installed: Option<String>,
    pub latest: Option<String>,
    pub compatible: bool,
    pub state: AvailabilityState,
    pub reason: String,
    pub channel: Option<String>,
}

impl AvailabilityItem {
    pub fn plugin(
        id: &str,
        installed: Option<&str>,
        latest: Option<&str>,
        channel: Option<&str>,
        compatible: bool,
        core: bool,
        decision: &UpdateAvailability,
    ) -> Self {
        let state = if core {
            AvailabilityState::CoreSilent
        } else if decision.reason == "incompatible" {
            AvailabilityState::Incompatible
        } else if decision.available {
            AvailabilityState::UpdateAvailable
        } else {
            AvailabilityState::UpToDate
        };
        Self {
            id: id.into(),
            kind: UpdateKind::Plugin,
            installed: installed.map(str::to_owned),
            latest: latest.map(str::to_owned),
            compatible,
            state,
            reason: decision.reason.into(),
            channel: channel.map(str::to_owned),
        }
    }

    pub fn release(
        id: &str,
        kind: UpdateKind,
        installed: &str,
        latest: &str,
        compatible: bool,
    ) -> Self {
        let decision = update_availability(
            Some(installed),
            UpdateSource::Channel("official"),
            Some(UpdateOffer {
                channel: "official",
                version: Some(latest),
                payload_version: Some(latest),
                compatible,
            }),
        );
        let mut item = Self::plugin(
            id,
            Some(installed),
            Some(latest),
            Some("official"),
            compatible,
            false,
            &decision,
        );
        item.kind = kind;
        item
    }
}
