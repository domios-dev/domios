use super::*;

#[test]
fn machine_access_and_memory_areas_round_trip_without_accepting_unknown_fields() {
    for access in ["none", "read_only", "folder", "full"] {
        let folder = std::env::temp_dir().to_string_lossy().into_owned();
        let value = serde_json::json!({"allowed":true,"scope":"opaque","allow_tab_requests":false,"machine_access":access,"folder":folder,"memory_areas":["project"]});
        let permission: Permission = serde_json::from_value(value).unwrap();
        permission.validate().unwrap();
        let stored = serde_json::to_value(&permission).unwrap();
        assert_eq!(stored["machine_access"], access);
        assert_eq!(stored["memory_areas"], serde_json::json!(["project"]));
        assert_eq!(
            serde_json::from_value::<Permission>(stored).unwrap(),
            permission
        );
    }
    let mut permission = Permission {
        machine_access: MachineAccess::Folder,
        ..Default::default()
    };
    assert_eq!(permission.validate(), Err(Error::InvalidRequest));
    permission.machine_access = MachineAccess::ReadOnly;
    permission.memory_areas = Some(vec!["\n".into()]);
    assert_eq!(permission.validate(), Err(Error::InvalidRequest));
}

#[test]
fn capability_defaults_keep_existing_metadata_readable_and_reject_relative_folders() {
    let mut permission: Permission = serde_json::from_value(
        serde_json::json!({"allowed":true,"scope":"opaque","allow_tab_requests":false}),
    )
    .unwrap();
    assert!(permission.allow_memory);
    assert!(permission.allow_files);
    assert!(permission.folder.is_none());
    assert_eq!(permission.machine_access, MachineAccess::ReadOnly);
    assert!(permission.memory_areas.is_none());
    permission.folder = Some("relative/folder".into());
    assert_eq!(permission.validate(), Err(Error::InvalidRequest));
    let folder = std::env::temp_dir();
    permission.folder = Some(folder.to_string_lossy().into_owned());
    assert!(permission.validate().is_ok());
    permission.allow_memory = false;
    permission.allow_files = false;
    let restored: Permission =
        serde_json::from_value(serde_json::to_value(permission).unwrap()).unwrap();
    assert!(!restored.allow_memory);
    assert!(!restored.allow_files);
}

#[test]
fn interactive_content_requires_an_explicit_owner_permission() {
    let mut permission: Permission = serde_json::from_value(
        serde_json::json!({"allowed":true,"scope":"opaque","allow_tab_requests":false}),
    )
    .unwrap();
    assert!(!permission.allow_interactive_content);
    permission.allow_interactive_content = true;
    let restored: Permission =
        serde_json::from_value(serde_json::to_value(permission).unwrap()).unwrap();
    assert!(restored.allow_interactive_content);
}

#[test]
fn scope_boundary_is_measured_in_utf8_bytes() {
    let mut permission = Permission {
        allowed: true,
        scope: "é".repeat(4096),
        allow_tab_requests: false,
        allow_interactive_content: false,
        ..Default::default()
    };
    assert!(permission.validate().is_ok());
    assert!(permission.hint(Kind::Question).contains(&permission.scope));
    permission.scope.push('x');
    assert!(permission.validate().is_err());
}

#[test]
fn deny_by_default_and_tab_requests_need_an_explicit_grant() {
    let denied = Permission::default();
    assert!(!denied.allows(Kind::Question));
    let mut granted = Permission {
        allowed: true,
        scope: "Public project status".into(),
        allow_tab_requests: false,
        allow_interactive_content: false,
        ..Default::default()
    };
    assert!(granted.allows(Kind::Question));
    assert!(!granted.allows(Kind::TabRequest));
    assert_eq!(
        granted.require(Kind::TabRequest),
        Err(Error::TabRequestsNotAllowed)
    );
    assert_eq!(denied.require(Kind::Question), Err(Error::PermissionDenied));
    granted.allow_tab_requests = true;
    assert!(granted.allows(Kind::TabRequest));
    assert_ne!(
        denied.scope_sha256().unwrap(),
        granted.scope_sha256().unwrap()
    );
    granted.scope.clear();
    assert!(granted.validate().is_err());
}
