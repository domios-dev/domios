//! Pure verifier for the detached Ed25519 entitlement grant. No IO and no clock: callers pass `now`.

pub mod activation;
pub mod jcs;
pub mod payload;
pub mod policy;
pub mod shadow;
pub mod text;
pub mod verdict;
pub mod verify;

pub use activation::{access_source, AccessSource, Activation, CompiledKey, Phase, ACTIVATION};
pub use payload::{
    Audience, Authority, Enforcement, GrantPayload, Label, Limits, Mode, PayloadDefect, Perks,
    Scope, ScopedLimit, TransitionReason, COMPILED_FREE_LIMITS,
};
pub use policy::{Policy, PolicyError, PolicySpec, TrustedKey};
pub use shadow::{shadow, ApplicationAccess, Divergence};
pub use verdict::{compiled_free, EffectiveAccess, TimeState, Verdict};
pub use verify::{
    verify, Accepted, Binding, DetachedGrant, Facet, Facets, Refusal, Rejected, RevisionOrder,
    VerifiedGrant,
};

#[cfg(test)]
mod tests;
