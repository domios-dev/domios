//! The daemon-side read of the in-app surface policies.

use super::notification::SurfaceDispatch;
use super::ServiceContext;
use crate::notification::bell::BellPolicy;
use crate::notification::desktop::DesktopPolicy;
use codeterm_types::notification::NotifyLevel;

impl ServiceContext {
    /// Read fresh per notification, so closing a cell in settings binds the next
    /// message rather than the next restart. Both in-app surfaces read the one
    /// grid, so neither can know a level or a column the other does not.
    pub(crate) fn resolve_surface_dispatch(
        &self,
        declared: Option<NotifyLevel>,
    ) -> SurfaceDispatch {
        let notifications = crate::config::load_config().notifications;
        SurfaceDispatch {
            declared,
            bell: BellPolicy::from_config(&notifications),
            desktop: DesktopPolicy::from_config(&notifications),
        }
    }
}
