//! Messaging bridge — routes external messages to the General Agent PTY.

pub mod traits;
pub mod manager;
pub mod response;
pub mod telegram;

pub use manager::BridgeManager;
