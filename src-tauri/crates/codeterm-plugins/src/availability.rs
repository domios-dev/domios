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
