use super::{RestApiPhase, RestApiStatus};
use url::{Host, Url};

pub(super) fn development_frontend(
    debug: bool,
    dev_url: Option<&Url>,
) -> Result<Option<Url>, String> {
    if !debug {
        return Ok(None);
    }
    let Some(url) = dev_url else {
        return Ok(None);
    };
    let loopback = match url.host() {
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        Some(Host::Domain(host)) => host == "localhost",
        None => false,
    };
    if !loopback
        || !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("Development frontend must use a loopback HTTP origin".into());
    }
    Ok(Some(url.clone()))
}

#[derive(Clone, Copy)]
pub(super) struct BootObservation {
    pub navigation: u64,
    pub listener_generation: u64,
    pub frontend_generation: u64,
    pub bundled_fallback: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum BootFailureAction {
    Ignore,
    BundledFallback,
    Unavailable,
}

pub(super) fn boot_failure_action(
    boot: BootObservation,
    navigation: u64,
    listener: &RestApiStatus,
    frontend_generation: u64,
) -> BootFailureAction {
    if boot.navigation != navigation
        || boot.listener_generation != listener.generation
        || boot.frontend_generation != frontend_generation
        || listener.phase != RestApiPhase::Listening
    {
        return BootFailureAction::Ignore;
    }
    if boot.bundled_fallback {
        BootFailureAction::Unavailable
    } else {
        BootFailureAction::BundledFallback
    }
}

#[cfg(test)]
mod tests;
