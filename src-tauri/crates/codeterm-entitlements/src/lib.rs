//! PLAN ENFORCEMENT under the GNU AGPL v3 (AGPL-3.0-only), Part 2, and the Terms of Service, Section 5.
//! Bypassing, patching or forging it, or distributing anything that does, ends your license,
//! gets your accounts banned and is pursued as copyright infringement. Read NOTICE.md first.
//!
//! Pure verifier for the detached Ed25519 entitlement grant. No IO and no clock: callers pass `now`.

pub mod activation;
pub mod authority;
pub mod floor;
pub mod gate;
pub mod jcs;
pub mod payload;
pub mod policy;
pub mod text;
pub mod verdict;
pub mod verify;

pub use activation::{Activation, CompiledKey, ACTIVATION, PRODUCTION, TEST};
pub use authority::{enforcement, enforcing, pinned, EntitlementAuthority, PinnedAuthority};
pub use floor::RevisionFloor;
pub use gate::{
    admit, LimitGate, LimitKey, LimitRefusal, LimitSummary, LockCandidate, LockSet, LockedRefusal,
    FREE_LIMIT_REACHED, PLAN_LIMIT_LOCKED,
};
pub use payload::{
    Audience, Authority, Enforcement, GrantPayload, Label, Limits, Mode, PayloadDefect, Perks,
    Scope, ScopedLimit, SignedLimits, TransitionReason, COMPILED_FREE_LIMITS,
    TAB_TEAM_LIMITS_POLICY_VERSION,
};
pub use policy::{Policy, PolicyError, PolicySpec, TrustedKey};
pub use verdict::{compiled_free, next_transition, EffectiveAccess, TimeState, Verdict};
pub use verify::{
    authenticate, verify, Accepted, Binding, DetachedGrant, Facet, Facets, Refusal, Rejected,
    RevisionOrder, VerifiedGrant, MAX_CLOCK_SKEW_SECS,
};

#[cfg(test)]
mod tests;
