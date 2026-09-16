//! CodeTerm Mesh — peer-to-peer networking between CodeTerm instances.

pub mod broadcast;
pub mod crown;
pub mod discovered;
pub mod discovery;
pub mod health;
pub mod identity;
pub mod manager;
pub mod paired_resolve;
pub mod pairing;
pub mod peer_services;
pub mod peers;
pub mod ports;
pub mod reachability;
pub mod serve_setup;
pub mod toggle;
pub mod transport;
pub mod types;
pub mod web_ui_endpoints;

use std::sync::Arc;

use parking_lot::Mutex;

pub use broadcast::{
    broadcast_announce, broadcast_introductions, broadcast_sync, refresh_local_addresses,
    refresh_local_addresses_blocking, refresh_local_addresses_blocking_force, AnnounceReport,
};
pub use manager::{MeshManager, PairingDelivery};
pub use ports::{LastActivityProvider, MeshEventSink};
pub use serve_setup::{
    disable_web_serve, enable_web_serve, probe_serve_status, reconcile_web_serve,
    tailscale_serve_https_url, ServeReconcileSnapshot, ServeResult, ServeStatus,
};
pub use toggle::{announce_enabled_mesh, apply_mesh_enabled};

pub type MeshState = Arc<Mutex<MeshManager>>;
