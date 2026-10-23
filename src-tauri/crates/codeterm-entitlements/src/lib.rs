//! Pure verifier for the detached Ed25519 entitlement grant. No IO and no clock: callers pass `now`.

pub mod activation;
pub mod gate;
pub mod jcs;
pub mod payload;
pub mod policy;
pub mod shadow;
pub mod text;
pub mod verdict;
pub mod verify;

pub use activation::{access_source, AccessSource, Activation, CompiledKey, Phase, ACTIVATION};
pub use gate::{
    admit, enforcing, LimitGate, LimitKey, LimitRefusal, LimitSummary, LockCandidate, LockSet,
    LockedRefusal, FREE_LIMIT_REACHED, PLAN_LIMIT_LOCKED,
};
pub use payload::{
    Audience, Authority, Enforcement, GrantPayload, Label, Limits, Mode, PayloadDefect, Perks,
    Scope, ScopedLimit, SignedLimits, TransitionReason, COMPILED_FREE_LIMITS,
    TAB_TEAM_LIMITS_POLICY_VERSION,
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
