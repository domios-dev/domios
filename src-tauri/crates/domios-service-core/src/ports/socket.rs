use std::sync::OnceLock;

use serde::Serialize;
use serde_json::Value;

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

pub type ServiceRpc =
    dyn Fn(&str, Option<Value>, &ServiceState) -> Result<Value, RpcError> + Send + Sync;
pub type MeshRpc =
    dyn Fn(&str, Option<Value>, &ServiceState, &str) -> Result<Value, RpcError> + Send + Sync;
pub type StartServer = dyn Fn(MuxState, PtyState, NotifState, OutputBufferState, ServiceState) -> Result<(), String>
    + Send
    + Sync;

/// Socket-owned RPC entry points, registered once at startup so HTTP routes never name socket internals.
pub struct RpcBridge {
    pub protocol_version: fn() -> u64,
    pub version_skew_gate: fn(Option<&Value>) -> Option<RpcError>,
    pub dispatch_service_rpc: Box<ServiceRpc>,
    pub dispatch_mesh_rpc: Box<MeshRpc>,
    pub recover_remote_attempts: fn(&ServiceState, &str, &str, u64, bool) -> Result<(), RpcError>,
    pub respawn_remote: fn(
        &Option<Value>,
        &ServiceState,
        &str,
        &str,
        &TaskExecutionAttempt,
        u64,
    ) -> Result<Value, RpcError>,
    pub run_dispatch_startup: fn(DispatchStartup, StartupReady),
    pub start_server: Box<StartServer>,
}

static BRIDGE: OnceLock<RpcBridge> = OnceLock::new();

pub fn register(bridge: RpcBridge) {
    let _ = BRIDGE.set(bridge);
}

pub fn bridge() -> &'static RpcBridge {
    BRIDGE
        .get()
        .expect("rpc bridge is registered before any RPC route is served")
}

#[cfg(any(test, feature = "test-support"))]
fn stub_error() -> RpcError {
    RpcError {
        code: -32000,
        message: "rpc bridge test stub".into(),
        data: None,
    }
}

/// Installs a bridge whose entry points fail, except remote-attempt recovery, which finds none.
#[cfg(any(test, feature = "test-support"))]
pub fn register_test_stub() {
    register(RpcBridge {
        protocol_version: || 1,
        version_skew_gate: |_| None,
        dispatch_service_rpc: Box::new(|_, _, _| Err(stub_error())),
        dispatch_mesh_rpc: Box::new(|_, _, _, _| Err(stub_error())),
        recover_remote_attempts: |_, _, _, _, _| Ok(()),
        respawn_remote: |_, _, _, _, _, _| Err(stub_error()),
        run_dispatch_startup: |_, _| {},
        start_server: Box::new(|_, _, _, _, _| Err(stub_error().message)),
    });
}

pub const REMOTE_TASK_DISPATCH_FEATURE: &str = "remote_task_dispatch";

#[cfg(test)]
mod tests {
    #[test]
    fn rpc_bridge_is_send_and_sync() {
        super::super::assert_port::<super::RpcBridge>();
    }
}
