#![recursion_limit = "256"]

pub mod attributed_screen;
pub mod dispatch;
pub mod multiplexer;
pub mod native_prompt;
pub mod pty;
pub mod resize_owner;
pub mod session;

#[cfg(test)]
mod test_support;

extern crate domios_build_info as codeterm_build_info; // rename-bridge
extern crate domios_paths as codeterm_paths; // rename-bridge
extern crate domios_provider_signals as codeterm_provider_signals; // rename-bridge
extern crate domios_storage as codeterm_storage; // rename-bridge
extern crate domios_types as codeterm_types; // rename-bridge
extern crate domios_util as codeterm_util; // rename-bridge
