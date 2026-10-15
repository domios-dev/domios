// Re-export the domain crate's service_plan surface
pub use codeterm_plan::service_plan::*;

// App-crate adapters (tauri/mem/mux deps)
pub mod adopt;
#[cfg(test)]
mod adopt_tests;
pub mod lane_port_impl;
pub mod search_port_impl;
pub mod sink_impl;
#[cfg(test)]
mod task_key_contract_tests;
pub mod task_key_port_impl;

// ServiceContext extensions implemented in the app crate
pub(crate) mod mission_control_ext;
#[cfg(test)]
mod mission_control_spend_tests;
#[cfg(test)]
mod mission_control_tests;
mod project_path;
mod relink_ext;
pub mod respawn_lane;
mod state_ext;
