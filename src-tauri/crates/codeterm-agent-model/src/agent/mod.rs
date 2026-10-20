pub mod attributed_screen;
pub mod chat_types;
pub use codeterm_agent_detect::classify_input;
pub use codeterm_agent_detect::command_detection;
pub mod detector;
pub mod keys;
pub mod ledger;
pub use codeterm_plugin::model_catalog;
pub mod models;
pub use codeterm_agent_detect::motion_decoder;
pub use codeterm_agent_detect::native_prompt;
pub use codeterm_agent_detect::offline_state_classifier;
pub use codeterm_agent_detect::permission_request;
pub use codeterm_agent_detect::prompt;
pub mod provider;
pub mod provider_identity;
#[cfg(test)]
mod question_removal_tests;
pub mod recovered_seat_auth;
pub mod registry;
pub mod registry_search;
pub use codeterm_agent_detect::semantic_marker;
pub mod shell_command_state;
pub use codeterm_agent_detect::signal_extractor;
#[cfg(test)]
mod classify_input_wiring_tests;
pub mod interactive_prompt;
pub mod kpi_ledger;
pub mod reports;
pub mod status;
pub mod structured_prompt;
pub mod stub_launch;
pub mod tab_state;
#[cfg(any(test, feature = "test-support"))]
pub mod test_provider;
pub mod usage_parser;
