//! Messaging bridge — routes external messages to the General Agent PTY.

pub mod inbound_buffer;
pub mod manager;
mod markdown;
pub mod response;
pub mod sanitize;
pub mod supervisor;
pub mod telegram;
pub mod text_hooks;
pub mod threads;
pub mod traits;
pub mod voice_transcribe;

pub use manager::BridgeManager;
pub use supervisor::{BridgeRuntimeState, BridgeSupervisor};
