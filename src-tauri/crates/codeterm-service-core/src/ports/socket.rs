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
pub struct RpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

pub struct DispatchStartup {
    pub app: AppHandle,
    pub svc: ServiceState,
    pub plan_id: String,
    pub task_id: String,
    pub tab_id: String,
    pub startup_attempt_id: String,
    pub attempt_id: String,
    pub parent_tab_id: String,
    pub worker_title: String,
    pub original_task: Task,
    pub original_plan_mode: PlanMode,
    pub lane_for_commit: TaskLaneMetadata,
    pub canonical_project_path: String,
    pub provenance: Option<WorkerLaneProvenance>,
    pub provisioned_lane: Option<ProvisionedWorkerLane>,
    pub lane_protection: LaneProtection,
    pub spawn_command: String,
}

pub type StartupReady = tokio::sync::oneshot::Receiver<Result<(), String>>;

/// Socket-owned RPC entry points, registered once at startup so HTTP routes never name socket internals.
pub struct RpcBridge {
    #[cfg(test)]
    pub protocol_version: fn() -> u64,
    pub version_skew_gate: fn(Option<&Value>) -> Option<RpcError>,
    pub dispatch_service_rpc: fn(&str, Option<Value>, &ServiceState) -> Result<Value, RpcError>,
    pub dispatch_mesh_rpc: fn(&str, Option<Value>, &ServiceState, &str) -> Result<Value, RpcError>,
    pub recover_remote_attempts: fn(&ServiceState, &str, &str, u64, bool) -> Result<(), RpcError>,
    pub respawn_remote: fn(
        &Option<Value>,
        &AppHandle,
        &ServiceState,
        &str,
        &str,
        &TaskExecutionAttempt,
        u64,
    ) -> Result<Value, RpcError>,
    pub run_dispatch_startup: fn(DispatchStartup, StartupReady),
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

pub fn register(bridge: RpcBridge) {
    let _ = BRIDGE.set(bridge);
}

pub fn bridge() -> &'static RpcBridge {
    #[cfg(test)]
    crate::socket::register_rpc_bridge();
    BRIDGE
        .get()
        .expect("rpc bridge is registered before any RPC route is served")
}

pub const REMOTE_TASK_DISPATCH_FEATURE: &str = "remote_task_dispatch";

#[cfg(test)]
mod tests {
    #[test]
    fn rpc_bridge_is_send_and_sync() {
        super::super::assert_port::<super::RpcBridge>();
    }
}
