use super::*;

#[test]
fn browser_profile_name_sanitizer_keeps_safe_chars() {
    assert_eq!(sanitize_profile_name("  work-Acme_2 "), "work-Acme_2");
    assert_eq!(
        sanitize_profile_name("../../etc/passwd"),
        "______etc_passwd"
    );
    assert_eq!(sanitize_profile_name("my profile.v2"), "my_profile_v2");
    assert_eq!(sanitize_profile_name("caf\u{e9} \u{1F600}"), "caf___");
    assert_eq!(sanitize_profile_name(""), "");
    assert_eq!(sanitize_profile_name("   "), "");
}

#[test]
fn browser_profile_name_sanitizer_caps_length() {
    assert_eq!(sanitize_profile_name(&"a".repeat(100)).len(), MAX_NAME_LEN);
}

#[test]
fn browser_profile_name_sanitizer_avoids_windows_device_names() {
    assert_eq!(sanitize_profile_name("nul"), "_nul");
    assert_eq!(sanitize_profile_name("COM1"), "_COM1");
    assert_eq!(sanitize_profile_name("console"), "console");
}

#[test]
fn browser_profile_owner_scope_survives_reopen_and_keeps_owners_separate() {
    let layout = ProfileLayout::new(PathBuf::from("data/browsers"));
    let first = ProfileKind::from_request(&owner_profile("a123"));
    let reopened = ProfileKind::from_request(&owner_profile("a123"));
    let other = ProfileKind::from_request(&owner_profile("b456"));
    assert!(first.is_persistent());
    assert_eq!(layout.dir_for(&first), layout.dir_for(&reopened));
    assert_ne!(layout.dir_for(&first), layout.dir_for(&other));
}

#[test]
fn browser_profile_kind_empty_name_is_clean_and_throwaway() {
    let clean = ProfileKind::from_request("  ");
    assert!(!clean.is_persistent());
    assert_eq!(clean.label(), "clean");
    let named = ProfileKind::from_request("work");
    assert!(named.is_persistent());
    assert_eq!(
        named,
        ProfileKind::Named {
            name: "work".into()
        }
    );
}

#[test]
fn browser_profile_layout_places_clean_and_named_dirs_under_one_root() {
    let layout = ProfileLayout::under_data_dir(Path::new("/home/u/.domios"));
    let named = layout.dir_for(&ProfileKind::Named {
        name: "work".into(),
    });
    let clean = layout.dir_for(&ProfileKind::Clean {
        session_id: "abc".into(),
    });
    assert_eq!(
        named,
        PathBuf::from("/home/u/.domios/browsers/profiles/work")
    );
    assert_eq!(
        clean,
        PathBuf::from("/home/u/.domios/browsers/sessions/abc")
    );
}

#[test]
fn browser_profile_layout_lists_only_stable_named_dirs() {
    let root = tempfile::tempdir().unwrap();
    let layout = ProfileLayout::new(root.path().to_path_buf());
    for name in ["work", "alpha", "has space"] {
        std::fs::create_dir_all(layout.profiles_dir().join(name)).unwrap();
    }
    std::fs::write(layout.profiles_dir().join("file"), b"x").unwrap();
    assert_eq!(
        layout.list_named(),
        vec!["alpha".to_string(), "work".to_string()]
    );
}

#[test]
fn foreign_profile_lock_names_the_profile_and_external_owner() {
    let dir = Path::new("/profiles/work");
    let error = foreign_profile_error(dir, "ProcessSingleton: failed to create SingletonLock");
    assert!(error.contains(&dir.display().to_string()));
    assert!(error.contains("Domios does not own"));
    assert_eq!(
        foreign_profile_error(dir, "binary missing"),
        "binary missing"
    );
}
