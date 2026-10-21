//! App update model and backend-free updater core, shared by the native client and the server.

pub mod application_host;
pub mod apply;
pub mod channel;
pub mod checker;
pub mod manifest;
pub mod receipt;
pub mod single_flight;
pub mod update_state;

pub use channel::Channel;
pub use manifest::Manifest;
pub use update_state::{InstallMethod, UpdatePhase, UpdateState};

pub type UpdateStateHandle = std::sync::Arc<parking_lot::Mutex<UpdateState>>;

extern crate domios_build_info as codeterm_build_info; // rename-bridge
extern crate domios_paths as codeterm_paths; // rename-bridge
extern crate domios_plugins as codeterm_plugins; // rename-bridge
extern crate domios_types as codeterm_types; // rename-bridge
