use super::{RestApiPhase, RestApiStatus};

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
