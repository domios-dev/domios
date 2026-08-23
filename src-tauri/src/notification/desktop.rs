//! The desktop counterpart of the Telegram push gate: the same closed category
//! set, the same fail-closed default. A notification that names no category
//! reaches the inbox but never the toast, so it cannot interrupt the user.

use codeterm_types::config::schema::NotificationConfig;
use codeterm_types::notification::NotifyCategory;

#[cfg(test)]
mod tests;

/// Proof that the gate admitted one notification. The field is private and
/// `admit` is the only constructor, so no caller can conjure a pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DesktopPass(NotifyCategory);

impl DesktopPass {
    pub fn category(self) -> NotifyCategory {
        self.0
    }
}

/// Why a notification did or did not reach the desktop sink. Every refusal
/// names its reason, so a suppressed toast is never a silent drop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopAdmission {
    Allowed(DesktopPass),
    /// The caller named no category. This is the fail-closed edge.
    Uncategorised,
    /// Desktop notifications are switched off entirely.
    DesktopOff,
    /// A real category the user chose not to be interrupted by.
    CategoryDisabled(NotifyCategory),
}

impl DesktopAdmission {
    /// The only way to obtain a pass, and therefore the only way to reach the
    /// desktop sink.
    pub fn pass(self) -> Option<DesktopPass> {
        match self {
            Self::Allowed(pass) => Some(pass),
            Self::Uncategorised | Self::DesktopOff | Self::CategoryDisabled(_) => None,
        }
    }
}

/// The user's standing answer to "what may interrupt me on this desktop".
/// Its default admits nothing, so a route that never opts in never toasts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DesktopPolicy {
    pub enabled: bool,
    pub allowed: Vec<NotifyCategory>,
}

impl DesktopPolicy {
    /// An unrecognised config entry is dropped, exactly as an unrecognised
    /// category on the wire is rejected — never widened into a pass-through.
    pub fn from_config(cfg: &NotificationConfig) -> Self {
        Self {
            enabled: cfg.desktop,
            allowed: cfg
                .agent_categories
                .iter()
                .filter(|choice| choice.enabled)
                .filter_map(|choice| NotifyCategory::parse(&choice.category))
                .collect(),
        }
    }

    /// The routes that never raise a toast today: internal diagnostics and the
    /// in-app provider-update prompt.
    pub fn silent() -> Self {
        Self::default()
    }

    pub fn allows(&self, category: NotifyCategory) -> bool {
        self.allowed.contains(&category)
    }
}

/// The single entry to the desktop sink. A caller that named no category is
/// refused here, so routing an agent through `codeterm notify` cannot widen
/// what reaches the user.
pub fn admit(declared: Option<NotifyCategory>, policy: &DesktopPolicy) -> DesktopAdmission {
    let Some(category) = declared else {
        return DesktopAdmission::Uncategorised;
    };
    if !policy.enabled {
        return DesktopAdmission::DesktopOff;
    }
    if !policy.allows(category) {
        return DesktopAdmission::CategoryDisabled(category);
    }
    DesktopAdmission::Allowed(DesktopPass(category))
}
