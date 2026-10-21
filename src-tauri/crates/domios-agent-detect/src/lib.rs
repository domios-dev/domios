pub mod classify_input;
pub mod command_detection;
pub mod motion_decoder;
pub mod native_prompt;
pub mod offline_state_classifier;
pub mod permission_request;
pub mod prompt;
pub mod semantic_marker;
pub mod shell_command_state;
pub mod signal_extractor;

#[cfg(test)]
mod test_support;

extern crate domios_paths as codeterm_paths; // rename-bridge
extern crate domios_plugin as codeterm_plugin; // rename-bridge
extern crate domios_pty as codeterm_pty; // rename-bridge
extern crate domios_types as codeterm_types; // rename-bridge
