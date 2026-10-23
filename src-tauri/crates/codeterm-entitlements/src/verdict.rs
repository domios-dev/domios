use crate::payload::{
    Authority, Enforcement, GrantPayload, Label, Limits, Perks, TransitionReason,
    COMPILED_FREE_LIMITS,
};

/// Half-open windows: each boundary instant belongs to the later state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeState {
    NotYetValid,
    Fresh,
    Stale,
    Expired,
}

impl TimeState {
    pub fn of(payload: &GrantPayload, now: u64) -> Self {
        if now < payload.not_before {
            Self::NotYetValid
        } else if now >= effective_end(payload) {
            Self::Expired
        } else if now < payload.refresh_after {
            Self::Fresh
        } else {
            Self::Stale
        }
    }

    pub fn retains_grant(self) -> bool {
        matches!(self, Self::Fresh | Self::Stale)
    }
}

/// The next instant the time state changes after `now`; `None` once the grant has expired.
pub fn next_transition(payload: &GrantPayload, now: u64) -> Option<u64> {
    [
        payload.not_before,
        payload.refresh_after,
        effective_end(payload),
    ]
    .into_iter()
    .filter(|instant| *instant > now)
    .min()
}

/// An earlier authoritative period end stops paid access before the signed expiry, with no tail.
fn effective_end(payload: &GrantPayload) -> u64 {
    payload
        .period_end
        .map_or(payload.expires_at, |end| end.min(payload.expires_at))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub state: TimeState,
    pub authority: Authority,
    pub label: Label,
    pub limits: Limits,
    pub capabilities: Vec<String>,
    pub perks: Perks,
    pub period_end: Option<u64>,
    pub revision: u64,
    pub transition_reason: Option<TransitionReason>,
    pub enforcement: Enforcement,
}

/// What the grant confers at the verdict's instant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectiveAccess {
    pub authority: Authority,
    pub limits: Limits,
    pub capabilities: Vec<String>,
    pub perks: Perks,
}

impl Verdict {
    pub fn of(payload: &GrantPayload, now: u64) -> Self {
        Self {
            state: TimeState::of(payload, now),
            authority: payload.authority,
            label: payload.label,
            limits: payload.effective_limits(),
            capabilities: payload.capabilities.clone(),
            perks: payload.perks,
            period_end: payload.period_end,
            revision: payload.revision,
            transition_reason: payload.transition_reason,
            enforcement: payload.enforcement,
        }
    }

    pub fn effective(&self) -> EffectiveAccess {
        if self.state.retains_grant() {
            EffectiveAccess {
                authority: self.authority,
                limits: self.limits,
                capabilities: self.capabilities.clone(),
                perks: self.perks,
            }
        } else {
            compiled_free()
        }
    }
}

pub fn compiled_free() -> EffectiveAccess {
    EffectiveAccess {
        authority: Authority::Free,
        limits: COMPILED_FREE_LIMITS,
        capabilities: Vec::new(),
        perks: Perks::default(),
    }
}
