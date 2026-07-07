//! App shim — re-exports the `codeterm-bridge` domain crate.

pub mod response_capture;

pub use codeterm_bridge::bridge::{
    inbound_buffer, manager, sanitize, supervisor, telegram, text_hooks, traits, voice_transcribe,
    BridgeManager, BridgeRuntimeState, BridgeSupervisor,
};
pub use codeterm_bridge::ports;
pub use codeterm_bridge::registry;

pub mod response {
    pub use super::response_capture::capture_response;
    pub use codeterm_bridge::bridge::response::{chunk_message, strip_ansi};
}
