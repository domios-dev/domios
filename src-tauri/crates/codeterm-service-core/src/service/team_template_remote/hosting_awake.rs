use std::sync::Arc;

use super::{AwakePort, HostingLeases};
use crate::ports::host::UiEvents;
use crate::service::ServiceContext;

/// The keepawake hosted-role leases hold, registered by the host at startup.
pub struct HostingKeepAwake(pub crate::KeepAwakeStateHandle);

/// Bounds a keepawake whose holders vanished without a release.
const LEASE_BOUND_SECS: u64 = 12 * 60 * 60;

struct KeepAwakeLeasePort {
    keepawake: crate::KeepAwakeStateHandle,
    ui: Arc<dyn UiEvents>,
}

impl AwakePort for KeepAwakeLeasePort {
    fn hold(&self) -> Result<bool, String> {
        if crate::keepawake::status(&self.keepawake).active {
            return Ok(false);
        }
        crate::keepawake::start(
            &self.keepawake,
            &self.ui,
            Some(LEASE_BOUND_SECS),
            Some(false),
            None,
            None,
        )
        .map(|_| true)
    }

    fn release(&self) -> Result<(), String> {
        crate::keepawake::stop(&self.keepawake, &self.ui)
    }
}

/// Holds this host awake for a hosted role; a keepawake failure is reported, never fatal.
pub fn acquire_hosting_lease(svc: &ServiceContext, holder: &str) -> String {
    let Some(awake) = svc.host_extension::<HostingKeepAwake>() else {
        return "unavailable: keepawake is not hosted here".into();
    };
    let port = Arc::new(KeepAwakeLeasePort {
        keepawake: awake.0.clone(),
        ui: svc.ui.clone(),
    });
    match HostingLeases::global().acquire(holder, port) {
        Ok(()) => "held".into(),
        Err(error) => {
            log::warn!("[peer-hosting] keepawake for {holder} unavailable: {error}");
            format!("unavailable: {error}")
        }
    }
}

/// Remote Workers this peer restored from its session hold their leases again.
pub fn restore_hosting_leases(svc: &ServiceContext) {
    let hosted: Vec<String> = svc
        .mux
        .read()
        .all_tabs()
        .filter(|tab| {
            tab.report_target_mesh_peer.is_some()
                && tab
                    .task_binding
                    .as_ref()
                    .is_some_and(|binding| binding.attempt_id.is_some())
        })
        .map(|tab| tab.id.clone())
        .collect();
    for tab_id in hosted {
        let status = acquire_hosting_lease(svc, &tab_id);
        log::debug!("[peer-hosting] restored lease for {tab_id}: {status}");
    }
}
