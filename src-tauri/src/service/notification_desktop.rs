//! The daemon-side read of the in-app surface policies.

use super::notification::SurfaceDispatch;
use super::ServiceContext;
use crate::notification::desktop::DesktopPolicy;
use codeterm_types::notification::NotifyLevel;

impl ServiceContext {
    /// Read fresh per notification, so closing a cell in settings binds the next
    /// message rather than the next restart. The toast is the only in-app
    /// surface with a policy: the panel takes every record unconditionally.
    pub(crate) fn resolve_surface_dispatch(
        &self,
        declared: Option<NotifyLevel>,
    ) -> SurfaceDispatch {
        let notifications = crate::config::load_config().notifications;
        SurfaceDispatch {
            declared,
            desktop: DesktopPolicy::from_config(&notifications),
        }
    }
}
