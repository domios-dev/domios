pub mod controller;
mod controller_idle;
mod controller_reconcile;
mod controller_storage;
mod controller_wake;
mod controller_warm;
mod enrichment;
pub mod held_sends;
pub mod idle_drain;
pub mod idle_driver;
pub mod placement;
pub mod ports;
pub mod reconcile;
pub mod state;
pub mod storage;
pub mod store;
pub mod types;
pub mod warm_bootstrap;
mod warnings;

use std::sync::Arc;

use crate::service::{background_work, ServiceContext};

pub use controller::{
    mint_request_id, CloudMachineController, CloudSendStatusObserver, CreateCloudMachineParams,
};
pub use controller_reconcile::{LocalReconcileReport, LocalRecordReconcileOutcome};
pub use held_sends::{
    evaluate_guest_link_readiness, should_retry_wake_once, CloudWakeQueue, FlushWriteOutcome,
    GuestLinkReadiness, GuestLinkReadinessInput, HeldCloudSend, HeldCloudSendState,
    LinkGenerationFlushTracker, SingleFlightWakes, WakeRouteConfirmation,
    DEFAULT_WAKE_HOLD_DEADLINE_MS, EXPIRED_NOT_DELIVERED_CODE, MIN_WAKE_RETRY_READINESS_BUDGET_MS,
    RELEASE_CONDITION_PEER_READY,
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
pub use reconcile::{
    apply_local_record_adoption, apply_local_record_delete, backend_machines_from_usage,
    plan_local_record_reconcile, BackendMachineEntry, BackendMatchReason, LocalReconcilePolicy,
    LocalRecordReconcileAction, LocalRecordReconcileIntent, DEFAULT_CREATE_MATCH_WINDOW_MS,
    DEFAULT_UNCERTAIN_CREATE_GRACE_MS,
};
pub use state::{
    apply_draining_truth, can_issue_enrollment_apply, infer_backend_truth_from_usage,
    ownership_labels, plan_draining_recovery, reconcile_provider_machines, transition,
    usage_reports_credits_exhausted, BackendDrainingTruth, DrainingReconcileIntent,
    DrainingRecoveryPlan, LifecycleEvent, ObservedProviderMachine, ObservedProviderState,
    ReconcileOutcome, LABEL_CREATE_REQUEST_ID,
};
pub use storage::{
    is_local_disk_full, validate_volume_upgrade_gb, CloudArchiveReceipt, CloudColdWakeParams,
    CloudDeleteMode, CloudDeleteReceipt, CloudMachineStorageUsage, CloudStorageTier,
    CloudStorageUsageSummary, CloudVolumeResizeReply, ProviderSnapshotResidual,
    AUX_KEY_ALLOCATED_GB, AUX_KEY_PURGE_AT, AUX_KEY_RECOVERABLE_ARCHIVE_ID, AUX_KEY_STORAGE_TIER,
    DEFAULT_COLD_PURGE_DAYS, DEFAULT_HOT_GB_MONTH_RATE_CENTS, DEFAULT_HOT_RETENTION_DAYS,
    MAX_VOLUME_SIZE_GB,
};
pub use store::{CloudMachineStore, CloudWakeQueueStore};
pub use types::{
    CloudCheckoutSession, CloudCostSummary, CloudEndpoint, CloudError, CloudLifecycleState,
    CloudMachineRecord, CloudOwnershipReceipt, CloudPlanAllowances, CloudPromoRedemption,
    CloudRate, CloudReceipt, CloudSleepMode, CloudUsagePack, CloudUsageSize, CloudUsageSummary,
    PendingProvisioningOp, PendingProvisioningRequest, CLOUD_INGRESS_PORT, CLOUD_WAKE_SEND_PREFIX,
    COST_WARNING_TEXT, DEFAULT_IDLE_SLEEP_MINUTES, DEFAULT_LEASE_RENEW_LEAD_MS,
    PROVIDER_PROXY_PORT,
};
pub use warm_bootstrap::{BackendCreatePhases, WarmBootstrapPhaseEvent};

#[derive(Clone)]
pub(crate) struct CloudMachinesHandle(Arc<CloudMachineController>);

impl ServiceContext {
    #[cfg(any(test, feature = "test-support"))]
    pub fn install_cloud_machines_for_test(&self, controller: Arc<CloudMachineController>) {
        assert!(
            self.set_ext(CloudMachinesHandle(controller)).is_ok(),
            "cloud_machines test controller must be installed before first access"
        );
    }

    pub fn cloud_machines(&self) -> Arc<CloudMachineController> {
        let handle = self.ext_or_init(|| {
            let controller_id = self
                .try_ext::<crate::MeshStateHandle>()
                .map(|mesh| mesh.lock().status().instance_id)
                .unwrap_or_else(|| "local-controller".into());
            let controller = CloudMachineController::open_default(controller_id);
            let service = self.weak_self();
            controller.bind_product_observer(Arc::new(move |event| {
                if let Some(svc) = service.upgrade() {
                    svc.record_product_event(event);
                }
            }));
            let status_service = self.weak_self();
            controller.bind_send_status_observer(Arc::new(move |record, send| {
                let Some(svc) = status_service.upgrade() else {
                    return;
                };
                let Some(peer_id) = record.and_then(|rec| rec.peer_instance_id.as_deref()) else {
                    return;
                };
                let mut status = send.to_chat_send_status();
                status.tab_id = format!("mesh:{peer_id}/{}", send.tab_id);
                svc.ui.emit(
                    "chat-send-status",
                    serde_json::to_value(&status).unwrap_or_default(),
                );
            }));
            CloudMachinesHandle(Arc::new(controller))
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
            if !background_work::running(self, tab_id).is_empty()
                || self.tab_has_observed_background_work(tab_id)
            {
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

    fn tab_has_observed_background_work(&self, tab_id: &str) -> bool {
        self.mux
            .read()
            .get_tab(tab_id)
            .is_some_and(|tab| !tab.provider_signals.background.is_empty())
    }

    fn tab_has_active_agent_turn(&self, tab_id: &str) -> bool {
        let (mux_active, pty_id) = {
            let mux = self.mux.read();
            match mux.get_tab(tab_id) {
                Some(tab) => {
                    let active = tab.agent_booting
                        || tab.agent_state.as_ref().is_some_and(|state| {
                            matches!(
                                state.state,
                                codeterm_types::pty::TimelineState::Working
                                    | codeterm_types::pty::TimelineState::Question
                            )
                        })
                        || tab.agent_runtime_state.as_ref().is_some_and(|state| {
                            state
                                .get("active_turn_started_at_ms")
                                .is_some_and(|v| !v.is_null())
                                || matches!(
                                    state.get("status").and_then(serde_json::Value::as_str),
                                    Some("working" | "running" | "streaming")
                                )
                        });
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
mod analytics_tests;
#[cfg(test)]
mod draining_recovery_tests;
#[cfg(test)]
mod endpoint_tests;
#[cfg(test)]
mod idle_driver_tests;
#[cfg(test)]
mod reconcile_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod wake_queue_tests;
