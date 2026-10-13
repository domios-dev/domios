mod capability;
mod wire;

pub use capability::*;
pub use wire::*;

pub const MAX_RUNTIME_VALUE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_RUNTIME_STATE_BYTES: usize = 256 * 1024;
pub const MAX_RUNTIME_BATCH: usize = 1024;
pub const MAX_RUNTIME_STEPS: usize = 32;
pub const RUNTIME_HOOK_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

pub type Operation = RuntimeOperation;
pub type OperationResult = RuntimeResult;
pub type AgentEvent = AgentRuntimeEvent;
pub type PendingRequest = PendingRuntimeRequest;
