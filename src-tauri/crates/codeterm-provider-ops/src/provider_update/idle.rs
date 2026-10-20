use super::{storage, ProviderUpdateHost};

pub(super) fn schedule_async(service: &impl ProviderUpdateHost, _tab: Option<String>) {
    let service = service.clone();
    super::spawn_utility("provider-update-idle", move || schedule(&service, None));
}

pub(super) fn schedule(service: &impl ProviderUpdateHost, _only_tab: Option<&str>) {
    if let Err(error) =
        storage::transact(|state| state.recover_interrupted(super::command::owner_alive))
    {
        log::error!("[provider-update] interrupted update recovery failed: {error}");
    }
    super::process_waiting(service);
}
