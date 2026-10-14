#[cfg(test)]
mod action_provenance;
pub use codeterm_notification_core::answer;
pub mod answer_delivery;
pub use codeterm_notification_core::authority;
pub use codeterm_notification_core::channel_ask;
pub use codeterm_notification_core::channel_placement;
pub mod channels;
pub mod os_notify;
pub(crate) mod vet;
pub use codeterm_notification_core::dedupe;
pub use codeterm_notification_core::deep_link;
#[cfg(test)]
mod defaults_tests;
pub use codeterm_notification_core::desktop;
pub use codeterm_notification_core::persist;
pub use codeterm_notification_core::push;
pub use codeterm_notification_core::reach;
pub use codeterm_notification_core::store;

use std::sync::Arc;

use parking_lot::Mutex;

use store::NotificationStore;

/// Shared notification state, thread-safe.
pub type NotifState = Arc<Mutex<NotificationStore>>;

#[cfg(test)]
mod channel_ask_integration_tests;
#[cfg(test)]
mod desktop_integration_tests;
#[cfg(test)]
mod push_body_tests;
