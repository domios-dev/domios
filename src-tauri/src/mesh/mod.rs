//! CodeTerm Mesh — peer-to-peer networking between CodeTerm instances.
//!
//! Architecture:
//! - IdentityManager — signing, authentication, keypair operations
//! - MeshManager — thin facade composing identity + peer/discovery state

pub mod broadcast;
pub mod crown;
pub mod discovered;
pub mod discovery;
pub mod health;
pub mod identity;
pub mod manager;
pub mod pairing;
pub mod peer_services;
pub mod peers;
pub mod transport;
pub mod types;

use std::sync::Arc;

use parking_lot::Mutex;

pub use broadcast::{
    broadcast_announce, broadcast_introductions, broadcast_sync, refresh_local_addresses,
    AnnounceReport,
};
pub use manager::MeshManager;

pub type MeshState = Arc<Mutex<MeshManager>>;
