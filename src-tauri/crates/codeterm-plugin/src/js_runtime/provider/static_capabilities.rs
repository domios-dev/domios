use super::*;

pub(super) struct Snapshot {
    pub(super) auth_status_probe: Option<provider::AuthStatusProbe>,
    pub(super) tracks_background_work: bool,
    pub(super) structured_chat: Option<bool>,
}

impl Snapshot {
    pub(super) fn capture(vm: &PluginVm) -> Self {
        Self {
            auth_status_probe: read::<Option<provider::AuthStatusProbe>>(vm, "authStatusProbe")
                .flatten(),
            tracks_background_work: read(vm, "tracksBackgroundWork").unwrap_or(false),
            structured_chat: super::super::vm::has_method(vm, "usesStructuredChat")
                .then(|| read(vm, "usesStructuredChat").unwrap_or(false)),
        }
    }
}

fn read<R: serde::de::DeserializeOwned>(vm: &PluginVm, method: &str) -> Option<R> {
    if !super::super::vm::has_method(vm, method) {
        return None;
    }
    match call_json(vm, method, &[]) {
        Ok(value) => Some(value),
        Err(error) => {
            log::warn!(
                "[plugin {}] {method} capability unavailable: {error}",
                vm.plugin_id()
            );
            None
        }
    }
}

#[cfg(test)]
mod tests;
