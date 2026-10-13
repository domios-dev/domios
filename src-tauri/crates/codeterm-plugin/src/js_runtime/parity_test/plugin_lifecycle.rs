use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::Value;

use crate::plugin_lifecycle::{
    deliver_permission_changed, permission_changed_event, PermissionChangeSource,
    PermissionChangedEvent, PluginLifecycle,
};

use super::super::host_api::PluginSandbox;
use super::super::manifest::PermissionsConfig;
use super::super::vm::{call_json, PluginVm};

const SOURCE: &str = r#"module.exports.default = {
    onPermissionChanged(event) { this.events.push(event); },
    events: [],
    received() { return this.events; }
};"#;

struct NativeLifecycle {
    allowed: bool,
    events: Mutex<Vec<PermissionChangedEvent>>,
}

impl NativeLifecycle {
    fn new(allowed: bool) -> Self {
        Self {
            allowed,
            events: Mutex::new(Vec::new()),
        }
    }

    fn received(&self) -> Value {
        serde_json::to_value(&*self.events.lock()).unwrap()
    }
}

impl PluginLifecycle for NativeLifecycle {
    fn id(&self) -> &str {
        "lifecycle-fixture"
    }
    fn permission_allowed(&self, permission: &str) -> bool {
        self.allowed && matches!(permission, "local_recording" | "network:api.test:443")
    }
    fn on_permission_changed(&self, event: &PermissionChangedEvent) -> Result<(), String> {
        self.events.lock().push(event.clone());
        Ok(())
    }
}

fn vm(allowed: bool, source: &str) -> PluginVm {
    let mut permissions = PermissionsConfig {
        local_recording: allowed,
        ..Default::default()
    };
    if allowed {
        permissions.network.allow.push("api.test:443".into());
    }
    let sandbox = PluginSandbox::restricted(Vec::new(), Vec::new())
        .with_identity_and_perms("lifecycle-fixture", &permissions);
    PluginVm::from_source_sandboxed("lifecycle-fixture", source.into(), None, Arc::new(sandbox))
        .unwrap()
}

fn received(vm: &PluginVm) -> Value {
    call_json(vm, "received", &[]).unwrap()
}

#[test]
fn native_and_quickjs_permission_events_share_payload_and_confinement() {
    let native = NativeLifecycle::new(true);
    let js = vm(true, SOURCE);
    for permission in ["local_recording", "network:api.test:443"] {
        let event =
            permission_changed_event(permission, true, PermissionChangeSource::Install).unwrap();
        assert!(!deliver_permission_changed(&native, "foreign-plugin", &event).unwrap());
        assert!(!deliver_permission_changed(&js, "foreign-plugin", &event).unwrap());
        assert!(deliver_permission_changed(&native, "lifecycle-fixture", &event).unwrap());
        assert!(deliver_permission_changed(&js, "lifecycle-fixture", &event).unwrap());
    }
    let actual = received(&js);
    assert_eq!(actual, native.received());
    assert_eq!(actual.as_array().unwrap().len(), 2);
    assert_eq!(actual[0]["permission"], "local_recording");
    assert_eq!(actual[0]["allowed"], true);
    assert_eq!(actual[0]["source"], "install");
}

#[test]
fn revoked_and_unknown_permissions_do_not_receive_events() {
    let granted =
        permission_changed_event("local_recording", true, PermissionChangeSource::Action).unwrap();
    let active_native = NativeLifecycle::new(true);
    let active_js = vm(true, SOURCE);
    assert!(deliver_permission_changed(&active_native, "lifecycle-fixture", &granted).unwrap());
    assert!(deliver_permission_changed(&active_js, "lifecycle-fixture", &granted).unwrap());
    assert_eq!(received(&active_js), active_native.received());
    assert_eq!(received(&active_js).as_array().unwrap().len(), 1);
    drop(active_js);
    let native = NativeLifecycle::new(false);
    let js = vm(false, SOURCE);
    for permission in ["local_recording", "network:api.test:443", "unknown"] {
        let event =
            permission_changed_event(permission, true, PermissionChangeSource::Action).unwrap();
        assert!(!deliver_permission_changed(&native, "lifecycle-fixture", &event).unwrap());
        assert!(!deliver_permission_changed(&js, "lifecycle-fixture", &event).unwrap());
    }
    let denied = PermissionChangedEvent {
        permission: "local_recording".into(),
        allowed: false,
        source: PermissionChangeSource::Settings,
    };
    assert!(
        !deliver_permission_changed(&NativeLifecycle::new(true), "lifecycle-fixture", &denied)
            .unwrap()
    );
    assert!(!deliver_permission_changed(&vm(true, SOURCE), "lifecycle-fixture", &denied).unwrap());
    assert_eq!(received(&js), native.received());
    assert_eq!(received(&js), serde_json::json!([]));
}

#[test]
fn missing_lifecycle_export_is_optional() {
    let js = vm(true, "module.exports.default = {};");
    let event = permission_changed_event("local_recording", true, PermissionChangeSource::Settings)
        .unwrap();
    assert!(deliver_permission_changed(&js, "lifecycle-fixture", &event).unwrap());
}
