use super::{PlanError, PlanStore};
use crate::service_plan::store_db::hot_write::AttemptTarget;

impl PlanStore {
    #[allow(clippy::too_many_arguments)]
    pub fn record_runtime_interruption(
        &self,
        plan: &str,
        task: &str,
        attempt: &str,
        tab: &str,
        session: &str,
        nonce: &str,
        epoch: u64,
        reason: &str,
        now: u64,
    ) -> Result<(), PlanError> {
        self.change_attempt(
            plan,
            task,
            AttemptTarget {
                tab,
                id: Some(attempt),
                session: Some((session, nonce)),
                live: true,
                ..Default::default()
            },
            now,
            |attempt| {
                let attempt = attempt
                    .ok_or_else(|| PlanError::Io("runtime interruption binding changed".into()))?;
                let state = &mut attempt.interruption;
                if state.runtime_nonce.as_deref() != Some(nonce)
                    || state.runtime_epoch != Some(epoch)
                {
                    state.interrupted_at = Some(now);
                    state.reason = Some(reason.into());
                    state.runtime_epoch = Some(epoch);
                    state.runtime_nonce = Some(nonce.into());
                    state.recovered_at = None;
                }
                Ok(Some(()))
            },
        )?
        .ok_or_else(|| PlanError::Io("runtime interruption binding changed".into()))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_runtime_recovery(
        &self,
        plan: &str,
        task: &str,
        attempt: &str,
        tab: &str,
        session: &str,
        new_nonce: &str,
        old_nonce: &str,
        epoch: u64,
        now: u64,
    ) -> Result<(), PlanError> {
        self.change_attempt(
            plan,
            task,
            AttemptTarget {
                tab,
                id: Some(attempt),
                session: Some((session, new_nonce)),
                live: true,
                ..Default::default()
            },
            now,
            |attempt| {
                let attempt = attempt
                    .ok_or_else(|| PlanError::Io("runtime recovery binding changed".into()))?;
                let state = &mut attempt.interruption;
                if state.runtime_nonce.as_deref() != Some(old_nonce)
                    || state.runtime_epoch != Some(epoch)
                {
                    return Err(PlanError::Io(
                        "runtime recovery interruption changed".into(),
                    ));
                }
                state.recovered_at.get_or_insert(now);
                Ok(Some(()))
            },
        )?
        .ok_or_else(|| PlanError::Io("runtime recovery binding changed".into()))
    }
}

#[cfg(test)]
mod tests;
