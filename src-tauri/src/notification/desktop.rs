//! The desktop counterpart of the Telegram push gate: the same closed level
//! set, the same fail-closed default. A notification that names no level
//! reaches the inbox but never the toast, so it cannot interrupt the user.

use codeterm_types::config::schema::NotificationConfig;
use codeterm_types::notification::{NotifyDestination, NotifyLevel};

#[cfg(test)]
mod tests;

/// Proof that the gate admitted one notification. The field is private and
/// `admit` is the only constructor, so no caller can conjure a pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopPass(NotifyLevel);

impl DesktopPass {
    pub fn level(self) -> NotifyLevel {
        self.0
    }
}

/// Why a notification did or did not reach the desktop sink. Every refusal
/// names its reason, so a suppressed toast is never a silent drop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopAdmission {
    Allowed(DesktopPass),
    /// The caller named no level. This is the fail-closed edge.
    Unlevelled,
    /// Desktop notifications are switched off entirely.
    DesktopOff,
    /// A real level the user chose not to be interrupted by.
    LevelDisabled(NotifyLevel),
}

impl DesktopAdmission {
    /// The only way to obtain a pass, and therefore the only way to reach the
    /// desktop sink.
    pub fn pass(self) -> Option<DesktopPass> {
        match self {
            Self::Allowed(pass) => Some(pass),
            Self::Unlevelled | Self::DesktopOff | Self::LevelDisabled(_) => None,
        }
    }
}

/// The user's standing answer to "what may interrupt me on this desktop".
/// Its default admits nothing, so a route that never opts in never toasts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DesktopPolicy {
    pub enabled: bool,
    pub allowed: Vec<NotifyLevel>,
}

impl DesktopPolicy {
    /// An unrecognised config entry is dropped, exactly as an unrecognised
    /// level on the wire is rejected — never widened into a pass-through.
    pub fn from_config(cfg: &NotificationConfig) -> Self {
        Self {
            enabled: cfg.desktop,
            allowed: cfg
                .agent_levels
                .iter()
                .filter(|choice| choice.allows(NotifyDestination::Desktop))
                .filter_map(|choice| NotifyLevel::parse(&choice.level))
                .collect(),
        }
    }

    /// The routes that never raise a toast today: internal diagnostics and the
    /// in-app provider-update prompt.
    pub fn silent() -> Self {
        Self::default()
    }

    pub fn allows(&self, level: NotifyLevel) -> bool {
        self.allowed.contains(&level)
    }
}

/// The single entry to the desktop sink. A caller that named no level is
/// refused here, so routing an agent through `codeterm notify` cannot widen
/// what reaches the user.
pub fn admit(declared: Option<NotifyLevel>, policy: &DesktopPolicy) -> DesktopAdmission {
    let Some(level) = declared else {
        return DesktopAdmission::Unlevelled;
    };
    if !policy.enabled {
        return DesktopAdmission::DesktopOff;
    }
    if !policy.allows(level) {
        return DesktopAdmission::LevelDisabled(level);
    }
    DesktopAdmission::Allowed(DesktopPass(level))
}
