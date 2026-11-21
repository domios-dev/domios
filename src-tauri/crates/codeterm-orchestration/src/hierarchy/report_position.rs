use serde_json::Value;

pub(super) fn sender_position(
    svc: &codeterm_service_core::service::ServiceContext,
    tab_id: &str,
) -> Value {
    codeterm_service_core::service::agent_runtime::report_position::sender_position(svc, tab_id)
}

#[cfg(test)]
mod tests;
