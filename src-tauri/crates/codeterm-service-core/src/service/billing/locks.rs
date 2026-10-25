use super::{LimitGate, LimitKey, LockedRefusal};
use crate::multiplexer::state::MuxLocks;
use crate::service::{MuxEmitCause, ServiceContext};

pub const ENTITLEMENT_CHANGED_EVENT: &str = "entitlement-changed";

impl ServiceContext {
    /// The single authority for "is this item locked"; dormant builds lock nothing.
    pub fn locks(&self) -> MuxLocks {
        self.mux.read().locks()
    }

    pub fn is_locked(&self, limit: LimitKey, id: &str) -> bool {
        self.locks().is_locked(limit, id)
    }

    /// Pre-write refusal for any input into a tab; checked before the ledger or the PTY sees it.
    pub fn tab_write_lock(&self, tab_id: &str) -> Option<LockedRefusal> {
        self.mux.read().tab_write_lock(tab_id)
    }

    pub fn tab_write_lock_delivery(
        &self,
        tab_id: &str,
    ) -> Result<(), crate::service::delivery_error::DeliveryError> {
        match self.tab_write_lock(tab_id) {
            Some(refusal) => Err(
                crate::service::delivery_error::DeliveryError::PlanLimitLocked {
                    tab: tab_id.to_string(),
                    detail: refusal.to_string(),
                },
            ),
            None => Ok(()),
        }
    }

    pub fn item_write_lock(&self, limit: LimitKey, id: &str) -> Option<LockedRefusal> {
        self.mux.read().item_write_lock(limit, id)
    }

    /// Run after anything that may change the entitlement snapshot; item-set changes need no
    /// call because every mux snapshot projects locks from its own items.
    pub fn sync_limit_gate(&self) {
        self.apply_limit_gate(self.limit_gate());
    }

    pub(crate) fn apply_limit_gate(&self, gate: LimitGate) {
        self.project_limit_gate(gate, false);
    }

    /// Deadline ticks always announce, so a UI deferring lock presentation learns checking ended.
    pub(crate) fn announce_limit_gate(&self) {
        self.project_limit_gate(self.limit_gate(), true);
    }

    fn project_limit_gate(&self, gate: LimitGate, announce: bool) {
        let summary = gate.summary();
        let change = self.mux.write().set_limit_gate(gate);
        if !change.gate && !announce {
            return;
        }
        if change.locks {
            self.emit_state_changed(MuxEmitCause::Capability);
        }
        self.ui.emit(
            ENTITLEMENT_CHANGED_EVENT,
            serde_json::json!({ "limits": summary, "locks": self.locks() }),
        );
    }
}

#[cfg(test)]
mod tests;
