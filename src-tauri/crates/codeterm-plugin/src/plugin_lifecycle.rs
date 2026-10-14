use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionChangeSource {
    Install,
    Action,
    #[default]
    Settings,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct PermissionChangedEvent {
    pub permission: String,
    pub allowed: bool,
    pub source: PermissionChangeSource,
}

pub fn permission_changed_event(
    permission: &str,
    allowed: bool,
    source: PermissionChangeSource,
) -> Option<PermissionChangedEvent> {
    allowed.then(|| PermissionChangedEvent {
        permission: permission.to_string(),
        allowed: true,
        source,
    })
}

pub trait PluginLifecycle: Send + Sync {
    fn id(&self) -> &str;
    fn permission_allowed(&self, permission: &str) -> bool;
    fn on_permission_changed(&self, event: &PermissionChangedEvent) -> Result<(), String>;
}

pub fn deliver_permission_changed(
    plugin: &dyn PluginLifecycle,
    target: &str,
    event: &PermissionChangedEvent,
) -> Result<bool, String> {
    if plugin.id() != target || !event.allowed || !plugin.permission_allowed(&event.permission) {
        return Ok(false);
    }
    plugin.on_permission_changed(event)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_produces_one_event_per_grant_and_not_now_produces_none() {
        for source in [
            PermissionChangeSource::Install,
            PermissionChangeSource::Action,
        ] {
            let events: Vec<_> = ["local_recording", "secrets"]
                .into_iter()
                .flat_map(|permission| permission_changed_event(permission, true, source))
                .collect();
            assert_eq!(events.len(), 2);
            assert_eq!(events[0].permission, "local_recording");
            assert_eq!(events[1].permission, "secrets");
            assert!(events
                .iter()
                .all(|event| event.source == source && event.allowed));
            assert!(permission_changed_event("local_recording", false, source).is_none());
        }
    }
}
