use super::{RestApiPhase, RestApiStatus};
use url::Url;

pub(super) fn external_link(url: &Url, origin: Option<&str>) -> Option<Url> {
    if !matches!(url.scheme(), "http" | "https" | "mailto")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host_str() == Some("tauri.localhost")
        || origin.is_some_and(|origin| url.origin().ascii_serialization() == origin)
    {
        return None;
    }
    Some(url.clone())
}

pub(super) use codeterm_service_core::startup::frontend_shell_development_frontend as development_frontend;

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
