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
    pub(crate) fn send_refused_report_diagnostic(
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
mod tests {
    use super::*;
    use crate::multiplexer::state::MultiplexerState;
    use crate::multiplexer::types::AgentRole;
    use crate::plan::{OrchestratorRef, Plan, TaskStatus};
    use crate::service::messenger::MessageKind;
    use crate::service::plan::NewTask;
    use crate::service::test_fixture::ServiceFixture;

    #[test]
    fn refused_report_diagnostic_is_never_stored_as_attempt_report() {
        let (mut mux, parent_tab_id) = MultiplexerState::new();
        let workspace_id = mux.find_workspace_for_tab(&parent_tab_id).unwrap();
        let worker_tab_id = mux.create_tab(&workspace_id).unwrap();
        mux.set_tab_role(&worker_tab_id, AgentRole::Worker, Some(&parent_tab_id))
            .unwrap();
        let fixture = ServiceFixture::new(mux);
        let mut plan = Plan::new(
            "Refused report diagnostic".into(),
            fixture.temp.path().display().to_string(),
            OrchestratorRef::from_tab(&parent_tab_id),
        );
        fixture.service.plan_store.save(&mut plan, 1).unwrap();
        let task_id = fixture
            .service
            .plan_store
            .add_task(
                &plan.id,
                NewTask {
                    name: "refused report task".into(),
                    kind: "implement".into(),
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        fixture
            .service
            .plan_store
            .assign_task(&plan.id, &task_id, &worker_tab_id, None, 3)
            .unwrap();
        let before = fixture.service.plan_store.show_task(&task_id).unwrap().task;
        let reason = "canonical checkout has uncommitted changes";
        let original = "STATUS: in_review\nCHANGES: completed slice";
        let mut received = None;

        let target = deliver_refused_report_diagnostic(
            &fixture.service.mux,
            &worker_tab_id,
            &format!("Reason: {reason}\n\nOriginal report text:\n{original}"),
            |recipient, envelope, body| {
                received = Some((recipient.to_string(), envelope, body.to_string()));
                Ok(())
            },
        )
        .unwrap();

        let (recipient, envelope, body) = received.unwrap();
        assert_eq!(target, parent_tab_id);
        assert_eq!(recipient, target);
        assert_eq!(envelope.kind, MessageKind::Report);
        assert_eq!(envelope.from_tab, worker_tab_id);
        let worker_title = fixture
            .service
            .mux
            .read()
            .get_tab(&worker_tab_id)
            .map(|tab| tab.title.clone())
            .unwrap();
        assert_eq!(envelope.from_label, worker_title);
        assert_eq!(envelope.context.as_deref(), Some("report_refused"));
        assert!(body.contains(reason));
        assert!(body.contains(original));
        let after = fixture.service.plan_store.show_task(&task_id).unwrap().task;
        assert_eq!(after.status, TaskStatus::Pending);
        assert_eq!(
            after.execution_attempts[0].report,
            before.execution_attempts[0].report
        );
        assert!(after.execution_attempts[0].report.is_none());
        assert_eq!(fixture.service.report_store.lock().count(&parent_tab_id), 0);
    }
}
