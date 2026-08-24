//! The bell leg of the levels-by-destinations grid.
//!
//! Two decisions are deliberately separate here. The *column* answers whether a
//! level may ring the bell at all; the *master switch* answers whether the bell
//! surface is drawn. A refused column means nothing arrives; a hidden surface
//! means nothing is drawn while every record is still kept, so flipping the
//! switch back reveals the complete log.

use codeterm_types::config::schema::NotificationConfig;
use codeterm_types::notification::{NotifyDestination, NotifyLevel};

#[cfg(test)]
mod tests;

/// Why a notification did or did not reach the bell. Every refusal names its
/// reason, so a missing bell event is never a silent drop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BellAdmission {
    Allowed,
    /// A real level the user routed away from the bell.
    LevelDisabled(NotifyLevel),
}

impl BellAdmission {
    /// Test-only: production matches the variants so a refusal can name its level.
    #[cfg(test)]
    pub fn reaches_bell(self) -> bool {
        matches!(self, Self::Allowed)
    }
}

/// The user's standing answer to "which levels may ring the bell". Unlike the
/// desktop policy this fails *open* for an unlevelled message: a message that
/// names no level is kept and rings, because the bell is the surface that never
/// interrupts and dropping it would lose the message entirely.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BellPolicy {
    pub allowed: Vec<NotifyLevel>,
}

impl BellPolicy {
    /// An unrecognised config row is dropped, exactly as an unrecognised level
    /// on the wire is rejected — never widened into a pass-through.
    pub fn from_config(cfg: &NotificationConfig) -> Self {
        Self {
            allowed: cfg
                .agent_levels
                .iter()
                .filter(|choice| choice.allows(NotifyDestination::Bell))
                .filter_map(|choice| NotifyLevel::parse(&choice.level))
                .collect(),
        }
    }

    /// Every level rings. The pre-grid behaviour, and what the routes that
    /// carry no level ride on.
    pub fn open() -> Self {
        Self {
            allowed: NotifyLevel::ALL.to_vec(),
        }
    }

    pub fn allows(&self, level: NotifyLevel) -> bool {
        self.allowed.contains(&level)
    }
}

/// The single entry to the bell surface. An unlevelled caller is admitted: the
/// bell is where a message with nothing to declare has always landed.
pub fn admit(declared: Option<NotifyLevel>, policy: &BellPolicy) -> BellAdmission {
    let Some(level) = declared else {
        return BellAdmission::Allowed;
    };
    if !policy.allows(level) {
        return BellAdmission::LevelDisabled(level);
    }
    BellAdmission::Allowed
}
