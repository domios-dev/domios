use super::*;

pub(in crate::service) struct InAppNotice {
    pub title: String,
    pub actions: Vec<codeterm_notification_core::contract::NotificationAction>,
    pub origin: NotificationOrigin,
    pub intent: PushIntent,
    pub include_push: bool,
}

impl InAppNotice {
    pub(in crate::service) fn neighbor_connection(title: String, event_key: String) -> Self {
        Self {
            title,
            actions: vec![],
            origin: NotificationOrigin::default(),
            intent: PushIntent {
                level: None,
                event_key: Some(event_key),
            },
            include_push: false,
        }
    }
}

pub(super) fn create(
    store: &NotifState,
    notice: InAppNotice,
    push_ready: bool,
    dispatch: PushDispatch<impl FnOnce(PushEnvelope) -> Result<(), String>>,
    resolve_tab: impl FnOnce(&str) -> Option<NotificationProvenance>,
    emit: impl FnMut(serde_json::Value),
) -> String {
    let declared = notice.intent.level;
    let push_enabled = notice.include_push && push_ready;
    create_notification_with_delivery(
        if push_enabled {
            NotificationAdmission::Explicit
        } else {
            NotificationAdmission::LocalEvent
        },
        store,
        |_, _| {},
        notice.title,
        None,
        Some("info"),
        Ask::offering(Some(notice.actions)),
        Some(false),
        notice.origin,
        resolve_tab,
        PushDispatch {
            intent: if push_enabled {
                dispatch.intent
            } else {
                PushIntent::default()
            },
            per_scope_limit: dispatch.per_scope_limit,
            deliver: move |envelope| (dispatch.deliver)(neighbor_notice_push(envelope)),
        },
        SurfaceDispatch {
            declared,
            desktop: crate::notification::desktop::DesktopPolicy::silent(),
        },
        emit,
    )
}

#[cfg(test)]
mod tests;
