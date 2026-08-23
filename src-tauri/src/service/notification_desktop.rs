//! The daemon-side read of the desktop gate's policy.

use super::ServiceContext;
use crate::notification::desktop::DesktopPolicy;

impl ServiceContext {
    /// Read fresh per notification, so turning a category off in settings binds
    /// the next message rather than the next restart.
    pub(crate) fn resolve_desktop_policy(&self) -> DesktopPolicy {
        DesktopPolicy::from_config(&crate::config::load_config().notifications)
    }
}
