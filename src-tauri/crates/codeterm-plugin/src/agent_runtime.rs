mod owned_context;
pub use owned_context::*;
mod authority;
mod children;
mod declarations;
pub use children::*;
mod host_mcp;
pub use host_mcp::{
    RuntimeHostMcp, RuntimeHostMcpDeclaration, RuntimeHostMcpHttp, NEIGHBOR_REPLY_FRIENDSHIP_ENV,
    NEIGHBOR_REPLY_MCP_NAME, NEIGHBOR_REPLY_MCP_PARAM, NEIGHBOR_REPLY_MCP_TOOL,
};
mod history;
pub use history::*;
mod background;
mod capability;
mod http_sse;
pub use http_sse::{RuntimeHttpRequestEvent, RuntimeHttpRoute, RuntimeHttpSse};
pub mod install_validation;
pub mod limits_source;
pub use limits_source::{RuntimeProfileLimitsSource, RuntimeRegistryDefaults};
pub mod owner_profiles;
pub use authority::*;
pub use background::*;
mod stream_window;
pub use stream_window::*;
mod validation;
mod wire;
pub use validation::*;

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
