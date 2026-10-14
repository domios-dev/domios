use std::sync::OnceLock;

use serde::Serialize;
use serde_json::Value;

use crate::app_runtime::AppHandle;
use crate::multiplexer::commands::MuxState;
use crate::notification::NotifState;
use crate::plan::{PlanMode, Task, TaskExecutionAttempt, TaskLaneMetadata};
use crate::service::{LaneProtection, ProvisionedWorkerLane, ServiceState, WorkerLaneProvenance};
use crate::{OutputBufferState, PtyState};

#[derive(Clone, Debug, Serialize)]
pub(crate) struct RpcError {
    pub(crate) code: i32,
    pub(crate) message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) data: Option<Value>,
}

pub(crate) struct DispatchStartup {
    pub(crate) app: AppHandle,
    pub(crate) svc: ServiceState,
    pub(crate) plan_id: String,
    pub(crate) task_id: String,
    pub(crate) tab_id: String,
    pub(crate) startup_attempt_id: String,
    pub(crate) attempt_id: String,
    pub(crate) parent_tab_id: String,
    pub(crate) worker_title: String,
    pub(crate) original_task: Task,
    pub(crate) original_plan_mode: PlanMode,
    pub(crate) lane_for_commit: TaskLaneMetadata,
    pub(crate) canonical_project_path: String,
    pub(crate) provenance: Option<WorkerLaneProvenance>,
    pub(crate) provisioned_lane: Option<ProvisionedWorkerLane>,
    pub(crate) lane_protection: LaneProtection,
    pub(crate) spawn_command: String,
}

pub(crate) type StartupReady = tokio::sync::oneshot::Receiver<Result<(), String>>;

/// Socket-owned RPC entry points, registered once at startup so HTTP routes never name socket internals.
pub(crate) struct RpcBridge {
    pub(crate) protocol_version: u64,
    pub(crate) version_skew_gate: fn(Option<&Value>) -> Option<RpcError>,
    pub(crate) dispatch_service_rpc:
        fn(&str, Option<Value>, &ServiceState) -> Result<Value, RpcError>,
    pub(crate) dispatch_mesh_rpc:
        fn(&str, Option<Value>, &ServiceState, &str) -> Result<Value, RpcError>,
    pub(crate) recover_remote_attempts:
        fn(&ServiceState, &str, &str, u64, bool) -> Result<(), RpcError>,
    pub(crate) respawn_remote: fn(
        &Option<Value>,
        &AppHandle,
        &ServiceState,
        &str,
        &str,
        &TaskExecutionAttempt,
        u64,
    ) -> Result<Value, RpcError>,
    pub(crate) run_dispatch_startup: fn(DispatchStartup, StartupReady),
    pub(crate) start_server: fn(
        MuxState,
        PtyState,
        NotifState,
        AppHandle,
        OutputBufferState,
        ServiceState,
    ) -> Result<(), String>,
}

static BRIDGE: OnceLock<RpcBridge> = OnceLock::new();

pub(crate) fn register(bridge: RpcBridge) {
    let _ = BRIDGE.set(bridge);
}

pub(crate) fn bridge() -> &'static RpcBridge {
    #[cfg(test)]
    crate::socket::register_rpc_bridge();
    BRIDGE
        .get()
        .expect("rpc bridge is registered before any RPC route is served")
}
