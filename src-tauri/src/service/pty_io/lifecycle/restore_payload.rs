use crate::multiplexer::types::{
    PendingDeliveryChannel, PendingDeliveryRecord, PendingDeliveryState,
};

pub(super) fn requires_pending_input(record: Option<&PendingDeliveryRecord>) -> bool {
    record.is_some_and(|record| {
        record.channel == PendingDeliveryChannel::InteractiveInput
            && record.state() == PendingDeliveryState::Pending
            && record.attempt_provenance().is_none()
    })
}

pub(super) fn resolve_restored_payload(
    record: Option<&PendingDeliveryRecord>,
    resolve: impl FnOnce() -> Result<Option<String>, String>,
) -> Result<Option<String>, String> {
    if !requires_pending_input(record) {
        return Ok(None);
    }
    let detail = match resolve() {
        Ok(_) => "Pending input is retained; automatic restore cannot replay it".to_string(),
        Err(error) => error,
    };
    Err(format!("restored_input_pending: {detail}"))
}

#[cfg(test)]
mod tests;
