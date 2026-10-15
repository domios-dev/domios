use codeterm_service_core::persisted_state::Incident;
use codeterm_service_core::service::ServiceState;

/// Loaders report while holding their own locks (the config cache among them),
/// so the notification is raised from a separate task, never inline.
pub(super) fn install_notifier(service: &ServiceState) {
    let service = service.clone();
    codeterm_service_core::persisted_state::set_incident_sink(move |incident| {
        let service = service.clone();
        let incident = incident.clone();
        tauri::async_runtime::spawn(async move { notify(&service, &incident) });
    });
}

fn notify(service: &ServiceState, incident: &Incident) {
    let key = incident.preserved.as_ref().unwrap_or(&incident.path);
    notify_once(
        service,
        format!("{} failed to load", incident.file_name()),
        Some(format!(
            "{}. The original is {}; {}.",
            incident.error,
            incident.kept_copy(),
            incident.outcome()
        )),
        key,
    );
}

pub(super) fn notify_storage_unavailable(
    service: &ServiceState,
    state_path: &std::path::Path,
    reason: &str,
) {
    notify_once(
        service,
        "Storage unavailable".to_string(),
        Some(format!(
            "{}; the original state files were left unchanged.",
            reason
        )),
        state_path,
    );
}

fn notify_once(
    service: &ServiceState,
    title: String,
    description: Option<String>,
    _event_path: &std::path::Path,
) {
    service.log_system_notice(title, description, Some("error"));
}
