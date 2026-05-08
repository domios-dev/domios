//! CodeTerm Mesh Client
//!
//! Lightweight mesh networking client for CodeTerm relay.
//! Provides Ed25519 signing/verification, peer communication, and shared types.

pub mod types;
pub mod crown;
pub mod transport;
pub mod error;

pub use types::*;
pub use crown::*;
pub use transport::*;
pub use error::*;