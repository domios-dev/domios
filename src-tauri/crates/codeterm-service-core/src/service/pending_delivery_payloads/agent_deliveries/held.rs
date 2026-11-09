use super::*;

pub(super) const READINESS_HOLD_MS: u64 = 180_000;

impl AgentDeliveryStore {
    pub(crate) fn set_pending_reason(
        &self,
        tab: &str,
        id: &str,
        reason: &str,
    ) -> Result<(), String> {
        self.update(tab, id, |record| {
            if record.state != AgentDeliveryState::Pending || record.terminal_failure.is_some() {
                return Ok(false);
            }
            let reason = format!("{reason}; exact own-session initial readiness releases this send after earlier admitted sends.");
            if record.pending_reason.as_ref() == Some(&reason) {
                return Ok(false);
            }
            record.pending_reason = Some(reason);
            Ok(true)
        })?;
        Ok(())
    }

    pub(crate) fn refuse_held(&self, tab: &str, id: &str, reason: &str) -> Result<bool, String> {
        self.update(tab, id, |record| {
            if record.state != AgentDeliveryState::Pending {
                return Err("send cannot be cancelled or refused: payload may already have been written; inspect send-status and never replay uncertain input".into());
            }
            if record.terminal_failure.is_some() {
                return Ok(false);
            }
            record.terminal_failure = Some(reason.into());
            record.pending_reason = None;
            Ok(true)
        })
    }
}

#[cfg(test)]
mod tests;
