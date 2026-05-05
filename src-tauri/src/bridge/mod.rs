//! Messaging bridge — routes external messages to the General Agent PTY.

pub mod traits;
pub mod inbound_buffer;
pub mod manager;
pub mod response;
pub mod sanitize;
pub mod telegram;
pub mod voice_transcribe;

pub use manager::BridgeManager;
