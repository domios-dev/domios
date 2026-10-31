//! App update model and backend-free updater core, shared by the native client and the server.

pub mod application_host;
pub mod apply;
pub mod channel;
pub mod checker;
pub mod container;
pub mod debian;
pub mod headless;
pub mod manifest;
pub mod native_handoff;
pub mod receipt;
pub mod single_flight;
pub mod update_state;

pub use channel::Channel;
pub use manifest::Manifest;
pub use update_state::{InstallMethod, UpdatePhase, UpdateState};

pub type UpdateStateHandle = std::sync::Arc<parking_lot::Mutex<UpdateState>>;
