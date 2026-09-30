use crate::service::{boot_stage::BootStage, ServiceContext};

#[cfg(test)]
#[path = "cwd_guard_tests.rs"]
mod tests;

/// Refuse a launch whose Tab cwd is not a directory on this host: the PTY layer
/// would start it in the home directory while the Tab still names the old path.
pub(super) fn refuse_launch_without_cwd(svc: &ServiceContext, tab_id: &str) -> Option<String> {
    let cwd = svc
        .mux
        .read()
        .get_tab(tab_id)?
        .cwd
        .clone()
        .filter(|cwd| !cwd.trim().is_empty())?;
    let problem = crate::service::spawn_cwd::cwd_problem(&cwd)?;
    let reason = format!("working directory {problem}; the agent was not launched");
    log::error!("[launch] Tab {tab_id} refused: {reason}");
    if let Some(tab) = svc.mux.write().get_tab_mut(tab_id) {
        tab.agent_booting = false;
    }
    svc.emit_boot_stage(tab_id, BootStage::SpawnFailed, Some(reason.as_str()));
    svc.create_notification_ext(
        "Agent not launched: working directory missing".into(),
        Some(format!(
            "{reason}. Restore the directory, then run `codeterm agent restart --tab {tab_id}`, or close the Tab."
        )),
        Some("error"),
        None,
        None,
        crate::service::notification::NotificationOrigin::local_tab(tab_id.to_string()),
        crate::service::notification::PushIntent::default(),
        None,
    );
    Some(reason)
}
