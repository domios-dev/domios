use super::{LimitGate, LimitKey};
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

    /// Run after anything that may change the entitlement snapshot; item-set changes need no
    /// call because every mux snapshot projects locks from its own items.
    pub fn sync_limit_gate(&self) {
        self.apply_limit_gate(self.limit_gate());
    }

    pub(crate) fn apply_limit_gate(&self, gate: LimitGate) {
        let summary = gate.summary();
        let change = self.mux.write().set_limit_gate(gate);
        if !change.gate {
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
