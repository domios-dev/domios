pub mod attributed_screen;
pub mod chat_types;
pub use codeterm_agent_detect::classify_input;
pub use codeterm_agent_detect::command_detection;
pub mod detector;
pub(crate) mod interactive_prompt;
pub mod keys;
pub mod kpi;
pub mod kpi_ledger;
pub mod ledger;
pub mod live_state;
pub mod model_binary_watch;
pub use codeterm_plugin::model_catalog;
pub mod models;
pub use codeterm_agent_detect::motion_decoder;
pub use codeterm_agent_detect::native_prompt;
pub use codeterm_agent_detect::offline_state_classifier;
pub use codeterm_agent_detect::permission_request;
pub mod plugin_registrar;
pub use codeterm_agent_detect::prompt;
pub mod provider;
pub(crate) mod provider_identity;
#[cfg(test)]
mod question_removal_tests;
pub mod recovered_seat_auth;
pub mod registry;
pub mod registry_search;
pub mod reports;
pub mod run_watcher;
pub use codeterm_agent_detect::semantic_marker;
pub mod shell_command_state;
pub use codeterm_agent_detect::signal_extractor;
pub mod state_reactor;
pub mod state_verdict;
#[allow(dead_code)]
pub mod status;
pub mod structured_prompt;
pub mod stub_launch;
pub mod tab_state;
#[cfg(test)]
pub mod test_provider;
pub mod usage_parser;

#[cfg(test)]
#[path = "classify_input_wiring_tests.rs"]
mod classify_input_wiring_tests;
