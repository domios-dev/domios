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
    Grace,
    PastGrace,
}

impl TimeState {
    pub fn of(payload: &GrantPayload, now: u64) -> Self {
        if now < payload.not_before {
            Self::NotYetValid
        } else if now < payload.refresh_after {
            Self::Fresh
        } else if now < payload.expires_at {
            Self::Stale
        } else if now < payload.grace_until {
            Self::Grace
        } else {
            Self::PastGrace
        }
    }

    pub fn retains_grant(self) -> bool {
        matches!(self, Self::Fresh | Self::Stale | Self::Grace)
    }
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
