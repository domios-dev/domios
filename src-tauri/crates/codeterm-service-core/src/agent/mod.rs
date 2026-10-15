#[cfg(any(test, feature = "test-support"))]
pub use codeterm_agent_model::agent::test_provider;
pub use codeterm_agent_model::agent::{
    attributed_screen, chat_types, classify_input, command_detection, detector, keys, ledger,
    model_catalog, models, motion_decoder, native_prompt, offline_state_classifier,
    permission_request, prompt, provider, provider_identity, recovered_seat_auth, registry_search,
    semantic_marker, shell_command_state, signal_extractor, status, structured_prompt, stub_launch,
    tab_state, usage_parser,
};
pub mod interactive_prompt;
pub mod kpi;
pub mod kpi_ledger;
pub mod live_state;
pub mod model_binary_watch;
pub mod plugin_registrar;
pub mod reports;
pub mod run_watcher;
pub mod state_reactor;
pub mod state_verdict;

pub mod registry;
