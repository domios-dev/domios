use crate::multiplexer::commands::MuxState;
use crate::service::message_gateway::{DeliveryClass, MessageBody, MessageOrigin, OutboundMessage};
use crate::service::messenger::MessageEnvelope;
use crate::service::ServiceContext;

pub(crate) fn deliver_refused_report_diagnostic(
    mux: &MuxState,
    reporter_tab_id: &str,
    message: &str,
    deliver: impl FnOnce(&str, MessageEnvelope, &str) -> Result<(), String>,
) -> Result<String, String> {
    let (recipient, reporter_label) = mux
        .read()
        .get_tab(reporter_tab_id)
        .and_then(|tab| {
            tab.parent_tab_id
                .as_deref()
                .filter(|parent| !parent.trim().is_empty() && *parent != reporter_tab_id)
                .map(|parent| (parent.to_string(), tab.title.clone()))
        })
        .ok_or_else(|| "refused report diagnostic has no live parent tab".to_string())?;
    let mut envelope = MessageEnvelope::report(reporter_label, reporter_tab_id);
    envelope.context = Some("report_refused".into());
    let body = format!("REFUSED REPORT DIAGNOSTIC\n{message}");
    deliver(&recipient, envelope, &body)?;
    Ok(recipient)
}

impl ServiceContext {
    pub fn send_refused_report_diagnostic(
        &self,
        reporter_tab_id: &str,
        message: &str,
    ) -> Result<String, String> {
        deliver_refused_report_diagnostic(
            &self.mux,
            reporter_tab_id,
            message,
            |recipient, envelope, body| {
                self.gateway
                    .submit(
                        self,
                        OutboundMessage {
                            target: recipient.to_string(),
                            body: MessageBody::Text {
                                body: body.to_string(),
                            },
                            envelope: Some(envelope),
                            class: DeliveryClass::AgentTraffic,
                            origin: MessageOrigin::LocalTab,
                            idempotency_key: None,
                        },
                    )
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            },
        )
    }
}

#[cfg(test)]
#[path = "report_diagnostics/tests.rs"]
mod tests;
