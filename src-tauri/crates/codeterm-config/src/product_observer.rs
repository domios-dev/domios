use crate::schema::AppConfig;
use parking_lot::RwLock;
use std::sync::{Arc, Weak};

pub type PluginEnabledObserver = Arc<dyn Fn(bool) + Send + Sync>;
struct Registration {
    state: Weak<parking_lot::Mutex<AppConfig>>,
    observer: PluginEnabledObserver,
}
static OBSERVERS: RwLock<Vec<Registration>> = RwLock::new(Vec::new());

pub fn bind_plugin_enabled_observer(state: &crate::ConfigState, observer: PluginEnabledObserver) {
    let mut registrations = OBSERVERS.write();
    registrations.retain(|entry| {
        entry.state.strong_count() > 0 && !entry.state.ptr_eq(&Arc::downgrade(state))
    });
    registrations.push(Registration {
        state: Arc::downgrade(state),
        observer,
    });
}

pub fn enabled_changes(before: &AppConfig, after: &AppConfig) -> Vec<bool> {
    after
        .plugins
        .iter()
        .filter_map(|(id, settings)| {
            let enabled = settings.get("enabled")?.as_bool()?;
            (before
                .plugins
                .get(id)
                .and_then(|old| old.get("enabled"))
                .and_then(serde_json::Value::as_bool)
                != Some(enabled))
            .then_some(enabled)
        })
        .collect()
}

pub(crate) fn publish(state: &parking_lot::Mutex<AppConfig>, changes: Vec<bool>) {
    let observer = OBSERVERS.read().iter().find_map(|entry| {
        let owner = entry.state.upgrade()?;
        std::ptr::eq(owner.as_ref(), state).then(|| entry.observer.clone())
    });
    if let Some(observer) = observer {
        for enabled in changes {
            observer(enabled);
        }
    }
}
