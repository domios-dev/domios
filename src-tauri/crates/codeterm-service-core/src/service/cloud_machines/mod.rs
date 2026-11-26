pub mod controller;
mod enrichment;
pub mod held_sends;
pub mod idle_drain;
pub mod idle_driver;
pub mod placement;
pub mod ports;
pub mod state;
pub mod store;
pub mod types;

use std::sync::Arc;

use crate::service::{background_work, ServiceContext};

pub use controller::{mint_request_id, CloudMachineController, CreateCloudMachineParams};
pub use held_sends::{
    CloudWakeQueue, FlushWriteOutcome, HeldCloudSend, HeldCloudSendState, SingleFlightWakes,
    DEFAULT_WAKE_HOLD_DEADLINE_MS, RELEASE_CONDITION_PEER_READY,
};
pub use idle_drain::{
    evaluate_idle_and_renewal, DrainToken, IdleDecision, IdleEvalInput, IdlePolicy,
    IdleSleepAction, LocalPeerDrainGate, PeerBusySnapshot,
};
pub use idle_driver::{
    spawn_cloud_idle_driver, CloudIdleDriverConfig, CloudIdleDriverHandle, IdleTickWaiter,
    TokioIdleTickWaiter, IDLE_DRIVER_CADENCE_MS,
};
pub use placement::{select_placement, CloudTarget, PlacementDecision};
pub use ports::{
    Clock, CloudEnrollmentPort, CloudPeerControlPort, CloudProvisioningPort, CloudRouteSpec,
    EnrollmentBinding, EnrollmentOutcome, ProvisionedLease, ProvisionedMachine,
    ProvisioningCreateRequest, SystemClock,
};
pub use state::{
    can_issue_enrollment_apply, ownership_labels, reconcile_provider_machines, transition,
    LifecycleEvent, ObservedProviderMachine, ObservedProviderState, ReconcileOutcome,
};
pub use store::{CloudMachineStore, CloudWakeQueueStore};
pub use types::{
    CloudCostSummary, CloudError, CloudLifecycleState, CloudMachineRecord, CloudOwnershipReceipt,
    CloudPromoRedemption, CloudRate, CloudReceipt, CloudSleepMode, CloudUsageSummary,
    PendingProvisioningOp, PendingProvisioningRequest, CLOUD_INGRESS_PORT, CLOUD_WAKE_SEND_PREFIX,
    COST_WARNING_TEXT, DEFAULT_IDLE_SLEEP_MINUTES, DEFAULT_LEASE_RENEW_LEAD_MS,
};

#[derive(Clone)]
pub(crate) struct CloudMachinesHandle(Arc<CloudMachineController>);

impl ServiceContext {
    #[cfg(any(test, feature = "test-support"))]
    pub fn install_cloud_machines_for_test(&self, controller: Arc<CloudMachineController>) {
        assert!(self.set_ext(CloudMachinesHandle(controller)).is_ok());
    }

    pub fn cloud_machines(&self) -> Arc<CloudMachineController> {
        let handle = self.ext_or_init(|| {
            let controller_id = self
                .try_ext::<crate::MeshStateHandle>()
                .map(|mesh| mesh.lock().status().instance_id)
                .unwrap_or_else(|| "local-controller".into());
            CloudMachinesHandle(Arc::new(CloudMachineController::open_default(
                controller_id,
            )))
        });
        Arc::clone(&handle.0)
    }

    pub fn cloud_peer_busy_snapshot(&self, attached_viewers: usize) -> PeerBusySnapshot {
        let tab_ids: Vec<String> = {
            let mux = self.mux.read();
            mux.snapshot().tabs.keys().cloned().collect()
        };
        let mut active_agent_turns = 0usize;
        let mut running_background_jobs = 0usize;
        let mut held_sends = 0usize;
        for tab_id in &tab_ids {
            if self.tab_has_active_agent_turn(tab_id) {
                active_agent_turns += 1;
            }
            if !background_work::running(self, tab_id).is_empty() {
                running_background_jobs += 1;
            }
            if self.chat_send_ledger.has_unsettled_update_input(tab_id) {
                held_sends += 1;
            }
        }
        held_sends = held_sends.saturating_add(self.cloud_machines().held_send_count());
        PeerBusySnapshot {
            active_agent_turns,
            running_background_jobs,
            held_sends,
            attached_viewers,
        }
    }

    fn tab_has_active_agent_turn(&self, tab_id: &str) -> bool {
        let (mux_active, pty_id) = {
            let mux = self.mux.read();
            match mux.get_tab(tab_id) {
                Some(tab) => {
                    let active = tab.agent_booting
                        || tab.agent_runtime_state.as_ref().is_some_and(|state| {
                            state
                                .get("active_turn_started_at_ms")
                                .is_some_and(|v| !v.is_null())
                                || matches!(
                                    state.get("status").and_then(serde_json::Value::as_str),
                                    Some("working" | "running" | "streaming")
                                )
                        })
                        || !tab.provider_signals.background.guarded_items().is_empty();
                    (active, tab.pty_id.clone())
                }
                None => (false, None),
            }
        };
        if mux_active {
            return true;
        }
        pty_id.is_some_and(|pty_id| {
            self.agent_tracker
                .lock()
                .get_status(&pty_id)
                .has_active_turn()
        })
    }
}

#[cfg(test)]
mod idle_driver_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod wake_queue_tests;
