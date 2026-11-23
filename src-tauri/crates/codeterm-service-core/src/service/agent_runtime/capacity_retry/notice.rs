use crate::service::message_gateway::{DeliveryClass, MessageBody, MessageOrigin, OutboundMessage};
use crate::service::messenger::MessageEnvelope;
use crate::service::ServiceContext;
use codeterm_types::agent::AgentRole;

pub(super) fn stopped(svc: &ServiceContext, tab_id: &str, nonce: &str, reason: &str) {
    publish(
        svc,
        tab_id,
        nonce,
        reason,
        "automatic-recovery-stopped",
        "Automatic recovery stopped",
        Some(format!("domios send --tab {tab_id} \"continue\"")),
    );
}

pub(in crate::service::agent_runtime) fn model_unavailable(
    svc: &ServiceContext,
    tab_id: &str,
    nonce: &str,
    reason: &str,
) {
    publish(
        svc,
        tab_id,
        nonce,
        reason,
        "model_unavailable",
        "Agent model unavailable",
        None,
    );
}

fn publish(
    svc: &ServiceContext,
    tab_id: &str,
    nonce: &str,
    reason: &str,
    event: &'static str,
    title: &str,
    recovery: Option<String>,
) {
    let context = {
        let mux = svc.mux.read();
        let Some(tab) = mux
            .get_tab(tab_id)
            .filter(|tab| tab.session_binding_nonce.as_deref() == Some(nonce))
        else {
            return;
        };
        (
            tab.role,
            tab.parent_tab_id.clone(),
            tab.task_binding.clone(),
        )
    };
    let (role, parent, binding) = context;
    let task = binding
        .as_ref()
        .and_then(|binding| binding.task_id.as_deref())
        .unwrap_or("none");
    let attempt = binding
        .as_ref()
        .and_then(|binding| binding.attempt_id.as_deref())
        .unwrap_or("none");
    let body = format!("Tab {tab_id}, task {task}, attempt {attempt}: {reason}");
    svc.events.publish_generic(
        event,
        serde_json::json!({
            "tab_id":tab_id,"task_id":task,"attempt_id":attempt,"reason":reason,
            "parent_tab_id":parent,"recovery":recovery
        }),
    );
    if let Some(parent) = parent
        .clone()
        .filter(|parent| role == Some(AgentRole::Worker) && parent != tab_id)
    {
        let svc = svc.clone();
        std::thread::spawn(move || {
            let result = svc.gateway.submit(
                &svc,
                OutboundMessage {
                    target: parent,
                    body: MessageBody::Text { body },
                    envelope: Some(MessageEnvelope::codeterm_reminder(event)),
                    class: DeliveryClass::SystemNotice,
                    origin: MessageOrigin::System,
                    idempotency_key: Some(format!("{event}:{}", uuid::Uuid::new_v4())),
                },
            );
            if let Err(error) = result {
                log::warn!("[capacity-retry] parent notice failed: {error:?}");
            }
        });
    } else if parent.is_none() {
        svc.create_notification_actions_reported(
            title.into(),
            Some(body),
            Some("error"),
            Default::default(),
            None,
            crate::service::notification::NotificationOrigin::default(),
            crate::service::notification::PushIntent {
                level: Some(crate::notification::push::NotifyLevel::Failure),
                event_key: Some(uuid::Uuid::new_v4().to_string()),
            },
            None,
        );
    }
}
