use crate::multiplexer::restore_policy::{restore_on_boot, RestoreDecision};
use crate::service::ServiceContext;

pub fn initialize(svc: &ServiceContext, active_window_hours: u64) {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64;
    let window_ms = active_window_hours.saturating_mul(3_600_000);
    let mut mux = svc.mux.write();
    let general_workspace = super::general_agent::general_workspace_id(&mux);
    let snapshot = mux.snapshot();
    for workspace in &snapshot.workspaces {
        for id in &workspace.tabs {
            if let Some(tab) = mux.get_tab_mut(id) {
                tab.frozen = tab.restored_from_snapshot
                    && !tab.protected
                    && restore_on_boot(
                        general_workspace.as_deref() == Some(workspace.id.as_str()),
                        tab.protected,
                        tab.last_active_at_ms,
                        now_ms,
                        window_ms,
                    ) == RestoreDecision::Freeze;
            }
        }
    }
}
