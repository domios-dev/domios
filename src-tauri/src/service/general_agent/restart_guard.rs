use super::GENERAL_ALIAS;
use crate::service::boot_stage::BootStage;
pub(super) fn permit(svc: &crate::ServiceState, mux: &crate::MuxState, force: bool) -> bool {
    let target = mux.read().resolve_tab_id(GENERAL_ALIAS);
    let Some(tab) = target else {
        return true;
    };
    let running = crate::service::background_work::running(svc, &tab);
    if let Err(error) = crate::service::background_work::restart_guard(running.clone(), force) {
        let reason = format!("restart refused: {error:?}");
        crate::service::provider_signals::warn(svc, &tab, reason.clone());
        svc.emit_boot_stage(&tab, BootStage::SpawnFailed, Some(&reason));
        return false;
    }
    if force {
        crate::service::background_work::force_notice(svc, &tab, &running);
    }
    true
}
