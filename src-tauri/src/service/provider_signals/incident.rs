use super::ServiceContext;
use crate::notification::push::NotifyLevel;
use crate::service::message_gateway::{DeliveryClass, MessageBody, MessageOrigin, OutboundMessage};
use crate::service::messenger::MessageEnvelope;
use crate::service::notification::{NotificationOrigin, PushIntent};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IncidentTransition {
    Enter,
    Recover,
    Unchanged,
}

pub(super) fn incident_transition(was_lost: bool, lost: bool) -> IncidentTransition {
    let result = match (was_lost, lost) {
        (false, true) => IncidentTransition::Enter,
        (true, false) => IncidentTransition::Recover,
        _ => IncidentTransition::Unchanged,
    };
    log::debug!("[provider-access] transition={result:?}");
    result
}

pub(super) fn parent_target(
    role: bool,
    parent: Option<String>,
    owner: Option<String>,
    tab: &str,
) -> Option<String> {
    if !role {
        return None;
    }
    parent.or(owner).filter(|target| target != tab)
}

pub(super) fn intent(tab: &str, nonce: &str) -> PushIntent {
    PushIntent {
        level: Some(NotifyLevel::ActionNeeded),
        event_key: Some(format!("provider-access-lost:{tab}:{nonce}")),
    }
}

pub(super) fn incident(
    svc: &ServiceContext,
    tab_id: &str,
    session: &str,
    nonce: &str,
    transition: IncidentTransition,
) {
    let context = {
        let mut mux = svc.mux.write();
        let Some(tab) = mux.get_tab_mut(tab_id).filter(|tab| {
            tab.agent_session_id.as_deref() == Some(session)
                && tab.session_binding_nonce.as_deref() == Some(nonce)
        }) else {
            log::debug!("[provider-access] incident stale tab={tab_id}");
            return;
        };
        if (transition == IncidentTransition::Enter) != tab.provider_signals.access_lost() {
            log::debug!("[provider-access] superseded transition tab={tab_id}");
            return;
        }
        (
            tab.title.clone(),
            tab.role_profile.clone().unwrap_or_else(|| {
                tab.role
                    .map(|role| role.label().to_string())
                    .unwrap_or_else(|| "plain tab".into())
            }),
            tab.role.is_some(),
            tab.parent_tab_id.clone(),
            tab.task_binding
                .as_ref()
                .and_then(|binding| binding.plan_id.clone()),
            tab.agent_type.clone().unwrap_or_default(),
            tab.provider_signals.access_incident.take(),
        )
    };
    let (title, role, role_tab, parent, plan, provider, previous_notice) = context;
    if let Some(id) = previous_notice {
        let resolved = svc.notif.lock().resolve_id(&id);
        if resolved {
            svc.announce_resolved(&[id]);
        }
    }
    if transition != IncidentTransition::Enter {
        log::debug!("[provider-access] recovered tab={tab_id}");
        return;
    }
    let machine = svc
        .bridge_sender_identity(tab_id)
        .map(|identity| identity.machine)
        .unwrap_or_default();
    let body = format!("Tab {title} ({tab_id}), role {role}, machine {machine}, provider {provider}: provider access lost. Work is blocked. Run /login in that tab, or use the provider's token setup command (Claude: claude setup-token). The work owner can respawn on another provider.");
    let id = svc.create_notification_ext(
        "Provider access lost".into(),
        Some(body.clone()),
        Some("error"),
        None,
        Some(true),
        NotificationOrigin::local_tab(tab_id.into()),
        intent(tab_id, nonce),
        None,
    );
    let retained = {
        let mut mux = svc.mux.write();
        if let Some(tab) = mux.get_tab_mut(tab_id).filter(|tab| {
            tab.agent_session_id.as_deref() == Some(session)
                && tab.session_binding_nonce.as_deref() == Some(nonce)
                && tab.provider_signals.access_lost()
        }) {
            tab.provider_signals.access_incident = Some(id.clone());
            true
        } else {
            false
        }
    };
    if !retained {
        let resolved = svc.notif.lock().resolve_id(&id);
        if resolved {
            svc.announce_resolved(&[id]);
        }
        log::debug!("[provider-access] recovered during notification tab={tab_id}");
        return;
    }
    let owner = plan
        .and_then(|plan| svc.plan_store.get(&plan))
        .and_then(|plan| plan.owner.tab_id);
    let target = parent_target(role_tab, parent, owner, tab_id);
    log::debug!("[provider-access] incident tab={tab_id} target={target:?} level=action_needed");
    svc.events.publish_generic("provider-access-lost", serde_json::json!({"tab_id":tab_id,"provider":provider,"session_id":session,"notification_id":id,"target":target}));
    if let Some(target) = target {
        let svc = svc.clone();
        std::thread::spawn(move || {
            let result = svc.gateway.submit(
                &svc,
                OutboundMessage {
                    target,
                    body: MessageBody::Text {
                        body: format!("[provider access lost] {body}"),
                    },
                    envelope: Some(MessageEnvelope::codeterm_reminder("provider-access-lost")),
                    class: DeliveryClass::SystemNotice,
                    origin: MessageOrigin::System,
                    idempotency_key: Some(format!("provider-access-lost:{id}")),
                },
            );
            log::debug!("[provider-access] parent route result={result:?}");
            if let Err(error) = result {
                log::warn!("[provider-access] parent route failed: {error:?}");
            }
        });
    }
}
