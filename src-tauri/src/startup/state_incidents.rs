use crate::notification::push::NotifyLevel;
use crate::persisted_state::Incident;
use crate::service::notification::{NotificationOrigin, PushIntent};
use crate::service::ServiceState;

/// Loaders report while holding their own locks (the config cache among them),
/// so the notification is raised from a separate task, never inline.
pub(super) fn install_notifier(service: &ServiceState) {
    let service = service.clone();
    crate::persisted_state::set_incident_sink(move |incident| {
        let service = service.clone();
        let incident = incident.clone();
        tauri::async_runtime::spawn(async move { notify(&service, &incident) });
    });
}

fn notify(service: &ServiceState, incident: &Incident) {
    let key = incident.preserved.as_ref().unwrap_or(&incident.path);
    service.create_notification_ext(
        format!("{} failed to load", incident.file_name()),
        Some(format!(
            "{}. The original is {}; {}.",
            incident.error,
            incident.kept_copy(),
            incident.outcome()
        )),
        Some("error"),
        None,
        Some(true),
        NotificationOrigin::default(),
        PushIntent {
            level: Some(NotifyLevel::ActionNeeded),
            event_key: Some(format!("persisted-state:{}", key.display())),
        },
        None,
    );
}
