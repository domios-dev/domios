#[cfg(test)]
mod action_provenance;
pub mod answer;
pub mod answer_delivery;
pub mod authority;
pub mod channel_ask;
pub mod channel_placement;
pub mod channels;
pub mod dedupe;
pub mod deep_link;
#[cfg(test)]
mod defaults_tests;
pub mod desktop;
pub mod persist;
pub mod push;
pub mod reach;
pub mod store;

use std::sync::Arc;

use parking_lot::Mutex;

use store::NotificationStore;

/// Shared notification state, thread-safe.
pub type NotifState = Arc<Mutex<NotificationStore>>;
