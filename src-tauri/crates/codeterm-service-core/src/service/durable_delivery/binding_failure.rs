use super::super::pending_delivery_payloads::agent_deliveries::AgentDeliveryState;
use super::*;

#[derive(Default)]
pub(super) struct BindingFailures {
    pub(super) records: HashMap<(String, String), String>,
    unresolved: HashMap<String, String>,
}

fn unbound_saved(record: &AgentDeliveryRecord) -> bool {
    record.state == AgentDeliveryState::Pending
        && record.saved_launch.is_none()
        && record.terminal_failure.is_none()
        && record
            .durable_delivery
            .as_ref()
            .is_some_and(|delivery| delivery.saved_target.is_some())
}

impl ServiceContext {
    pub(super) fn binding_failure(
        &self,
        target: &str,
        record: &AgentDeliveryRecord,
    ) -> Option<String> {
        if !unbound_saved(record) {
            return None;
        }
        self.durable_runtime()
            .binding_failures
            .lock()
            .records
            .get(&(target.into(), record.id.clone()))
            .cloned()
    }

    pub(super) fn remember_binding_failure(&self, target: &str, cause: &str) {
        let owner = &self.pending_delivery_payloads.agent_deliveries;
        let snapshot = owner.records(target);
        let mut failures = self.durable_runtime().binding_failures.lock();
        match snapshot {
            Ok(records) => {
                for record in records.iter().filter(|record| unbound_saved(record)) {
                    failures
                        .records
                        .entry((target.into(), record.id.clone()))
                        .or_insert_with(|| cause.into());
                }
            }
            Err(_) => {
                failures
                    .unresolved
                    .entry(target.into())
                    .or_insert_with(|| cause.into());
            }
        }
    }

    pub(super) fn scope_pending_binding_failure(
        &self,
        target: &str,
        records: &[AgentDeliveryRecord],
    ) {
        let mut failures = self.durable_runtime().binding_failures.lock();
        if let Some(cause) = failures.unresolved.remove(target) {
            for record in records.iter().filter(|record| unbound_saved(record)) {
                failures
                    .records
                    .entry((target.into(), record.id.clone()))
                    .or_insert_with(|| cause.clone());
            }
        }
    }

    pub(super) fn resolve_pending_binding_failure(&self, target: &str) -> Result<(), String> {
        if !self
            .durable_runtime()
            .binding_failures
            .lock()
            .unresolved
            .contains_key(target)
        {
            return Ok(());
        }
        let records = self
            .pending_delivery_payloads
            .agent_deliveries
            .records(target)?;
        self.scope_pending_binding_failure(target, &records);
        Ok(())
    }
}
