//! Two suppressions that look alike and are not. `DeliveryLedger` answers "is
//! this the same message?" by identity. `ConditionLedger` answers "did this
//! condition just become true?" by transition. Neither reads a clock.

use std::collections::{HashMap, HashSet};

use super::push::scope_key;
use super::store::{NotificationProvenance, NotificationSeverity};

#[cfg(test)]
mod tests;

/// What makes two notifications the same message: the same words, at the same
/// severity, from the same origin. Derived, so no caller can opt out by omitting
/// a key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NotificationIdentity {
    scope: String,
    severity: NotificationSeverity,
    title: String,
    body: String,
}

impl NotificationIdentity {
    pub fn of(
        provenance: &NotificationProvenance,
        severity: &NotificationSeverity,
        title: &str,
        body: Option<&str>,
    ) -> Self {
        Self {
            scope: scope_key(provenance),
            severity: severity.clone(),
            title: title.trim().to_string(),
            body: body.unwrap_or_default().trim().to_string(),
        }
    }

    #[cfg(test)]
    pub fn scope(&self) -> &str {
        &self.scope
    }

    #[cfg(test)]
    pub fn severity(&self) -> &NotificationSeverity {
        &self.severity
    }

    #[cfg(test)]
    pub fn title(&self) -> &str {
        &self.title
    }

    #[cfg(test)]
    pub fn body(&self) -> &str {
        &self.body
    }
}

/// Outcome of identity admission. Both variants name a durable record, so a
/// suppressed duplicate still leaves the caller with an id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryAdmission {
    Deliver(String),
    Duplicate(String),
}

impl DeliveryAdmission {
    pub fn id(&self) -> &str {
        match self {
            Self::Deliver(id) | Self::Duplicate(id) => id,
        }
    }

    /// Only a first arrival reaches the native toast and the bell event.
    pub fn reaches_sink(&self) -> bool {
        matches!(self, Self::Deliver(_))
    }
}

/// One live entry per unread record. An identity leaves when its record is
/// read, cleared, evicted, or its scope closes — never because time passed.
#[derive(Default)]
pub struct DeliveryLedger {
    live: HashMap<NotificationIdentity, String>,
}

impl DeliveryLedger {
    pub fn existing(&self, identity: &NotificationIdentity) -> Option<String> {
        self.live.get(identity).cloned()
    }

    pub fn bind(&mut self, identity: NotificationIdentity, notification_id: String) {
        self.live.insert(identity, notification_id);
    }

    pub fn release(&mut self, notification_id: &str) {
        self.live.retain(|_, live| live != notification_id);
    }

    pub fn release_scope(&mut self, scope: &str) {
        self.live.retain(|identity, _| identity.scope != scope);
    }

    pub fn clear(&mut self) {
        self.live.clear();
    }

    #[cfg(test)]
    pub fn live_len(&self) -> usize {
        self.live.len()
    }
}

/// Whether an observation of a standing condition is news.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionTransition {
    Announce,
    AlreadyAnnounced,
}

/// A standing condition stays true for as long as it is true, so re-observing
/// it is the same news, not new news. Only the absent -> present edge announces;
/// the latch is released by an observed resolution or a closing scope.
#[derive(Default)]
pub struct ConditionLedger {
    announced: HashSet<(String, String)>,
}

impl ConditionLedger {
    pub fn observe_present(&mut self, scope: &str, condition: &str) -> ConditionTransition {
        if self
            .announced
            .insert((scope.to_string(), condition.to_string()))
        {
            ConditionTransition::Announce
        } else {
            ConditionTransition::AlreadyAnnounced
        }
    }

    pub fn observe_absent(&mut self, scope: &str, condition: &str) {
        self.announced
            .remove(&(scope.to_string(), condition.to_string()));
    }

    /// A closing tab takes its latches with it. Tab ids are not reused, so this
    /// cannot re-arm a live condition.
    pub fn close_scope(&mut self, scope: &str) {
        self.announced.retain(|(owner, _)| owner != scope);
    }
}

/// "An update is available" is a state, not an event: it stays true until the
/// update is installed.
pub const PROVIDER_UPDATE_AVAILABLE: &str = "provider-update-available";

pub fn provider_update_condition(provider_id: &str) -> String {
    format!("{PROVIDER_UPDATE_AVAILABLE}:{provider_id}")
}
