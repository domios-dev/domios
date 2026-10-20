pub use codeterm_notifications::notification::vet::{
    created_result, parse_push_intent, vet_actions, vet_target_tab, vet_title, CreateNotifBody,
};

pub fn vet_offer(
    svc: &crate::service::ServiceState,
    offer: Option<String>,
    offer_accept: Option<String>,
    actions: Option<&[codeterm_notification_core::contract::CallerAction]>,
    level: Option<crate::notification::push::NotifyLevel>,
) -> Result<Option<crate::notification::authority::SurfaceOffer>, String> {
    codeterm_notifications::notification::vet::vet_offer(
        |named| svc.resolve_surface_target(named),
        offer,
        offer_accept,
        actions,
        level,
    )
}
