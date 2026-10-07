pub mod background_work;
pub mod chat_types;
pub mod context;
pub mod keys;
pub mod models;
pub mod provider_signals;
pub mod roles;
pub mod session_model;
pub use roles::{
    is_legacy_cartographer_worker_binding, restore_role, AgentRole, ReportOutcomeClass,
    ReportStatus, RestoredRole,
};
pub use session_model::SessionModelTranscriptBoundary;
pub mod role_identity;
pub mod role_kind;
pub use role_kind::{RoleKindCapability, RoleKindDescriptor, RoleKindOrigin};

pub mod capability_probe;
