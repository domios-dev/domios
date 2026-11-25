use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;

use super::{
    authorized_cwd, eligible_cwd_with_root, for_tab_with_root, is_domios_owned_directory,
    tab_provenance_authorized,
};
use crate::agent::test_provider::MockProvider;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;
use codeterm_plugin::provider_account::{with_context, InvocationContext};

#[test]
fn directory_pre_trust_requires_authority_and_the_exact_launch_cwd() {
    for (authorized, tab, launch, expected) in [
        (true, Some("/exact"), Some("/exact"), Some("/exact")),
        (true, Some("/exact"), None, Some("/exact")),
        (false, Some("/exact"), Some("/exact"), None),
        (true, Some("/exact"), Some("/exact/child"), None),
        (true, None, Some("/exact"), None),
        (true, Some(""), None, None),
    ] {
        assert_eq!(authorized_cwd(authorized, tab, launch), expected);
    }
}

#[test]
fn domios_data_root_and_children_are_owned_while_outside_and_siblings_are_not() {
    let temp = tempfile::tempdir().unwrap();
    let domios_root = temp.path().join(".domios");
    let general = domios_root.join("general");
    let pad = domios_root.join("runtime").join("agent-pads").join("tab-1");
    let workspace_child = domios_root
        .join("provisioned-workspaces")
        .join("ws-1")
        .join("nested");
    let sibling = temp.path().join(".domios-other");
    let outside = temp.path().join("user-project");
    let file_inside = domios_root.join("config.toml");

    for dir in [
        &domios_root,
        &general,
        &pad,
        &workspace_child,
        &sibling,
        &outside,
    ] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(&file_inside, "").unwrap();

    assert!(is_domios_owned_directory(&domios_root, &domios_root));
    assert!(is_domios_owned_directory(&general, &domios_root));
    assert!(is_domios_owned_directory(&pad, &domios_root));
    assert!(is_domios_owned_directory(&workspace_child, &domios_root));

    assert!(!is_domios_owned_directory(&sibling, &domios_root));
    assert!(!is_domios_owned_directory(&outside, &domios_root));
    assert!(!is_domios_owned_directory(&file_inside, &domios_root));
    assert!(!is_domios_owned_directory(
        &domios_root.join("..").join("user-project"),
        &domios_root
    ));
    assert!(!is_domios_owned_directory(
        &domios_root.join("missing"),
        &domios_root
    ));
}

#[cfg(unix)]
#[test]
fn symlink_inside_domios_root_pointing_outside_is_not_domios_owned() {
    let temp = tempfile::tempdir().unwrap();
    let domios_root = temp.path().join(".domios");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&domios_root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let escape = domios_root.join("escape");
    std::os::unix::fs::symlink(&outside, &escape).unwrap();

    assert!(!is_domios_owned_directory(&escape, &domios_root));
}

#[test]
fn domios_owned_cwd_is_eligible_without_provenance_and_outside_keeps_provenance_rules() {
    let temp = tempfile::tempdir().unwrap();
    let domios_root = temp.path().join(".domios");
    let child = domios_root.join("custom-subdir");
    let outside = temp.path().join("project");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::create_dir_all(&outside).unwrap();

    let root_str = domios_root.to_str().unwrap();
    let child_str = child.to_str().unwrap();
    let outside_str = outside.to_str().unwrap();

    assert_eq!(
        eligible_cwd_with_root(false, Some(root_str), Some(root_str), &domios_root),
        Some(root_str)
    );
    assert_eq!(
        eligible_cwd_with_root(false, Some(child_str), Some(child_str), &domios_root),
        Some(child_str)
    );
    assert_eq!(
        eligible_cwd_with_root(false, Some(child_str), Some(root_str), &domios_root),
        None
    );
    assert_eq!(
        eligible_cwd_with_root(false, Some(outside_str), Some(outside_str), &domios_root),
        None
    );
    assert_eq!(
        eligible_cwd_with_root(true, Some(outside_str), Some(outside_str), &domios_root),
        Some(outside_str)
    );
}

#[test]
fn pre_trust_dispatches_for_domios_dirs_and_selected_account_only() {
    let temp = tempfile::tempdir().unwrap();
    let domios_root = temp.path().join(".domios");
    let child = domios_root.join("workspaces").join("fresh");
    let outside = temp.path().join("user-repo");
    let default_home = temp.path().join("default-home");
    let named_home = domios_root
        .join("provider-accounts")
        .join("fixture")
        .join("work");
    for dir in [&domios_root, &child, &outside, &default_home, &named_home] {
        std::fs::create_dir_all(dir).unwrap();
    }

    let log = Arc::new(Mutex::new(Vec::new()));
    let (mut mux, tab_id) = MultiplexerState::new();
    mux.get_tab_mut(&tab_id).unwrap().agent_type = Some("fixture".into());
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::new("fixture").with_pre_trust_log(log.clone()),
    ));

    let run_for_cwd = |cwd: &Path| {
        let cwd_str = cwd.to_string_lossy().to_string();
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&tab_id)
            .unwrap()
            .cwd = Some(cwd_str.clone());
        for_tab_with_root(
            &fixture.service,
            &fixture.service.agent_registry,
            "fixture",
            &tab_id,
            Some(&cwd_str),
            &domios_root,
        );
    };

    // Outside user folder without provenance/team is never auto-trusted.
    run_for_cwd(&outside);
    assert!(log.lock().is_empty());

    // Domios root and child are pre-trusted automatically without provenance or team_id.
    run_for_cwd(&domios_root);
    run_for_cwd(&child);
    assert_eq!(
        *log.lock(),
        vec![
            (domios_root.to_string_lossy().to_string(), None),
            (child.to_string_lossy().to_string(), None),
        ]
    );
    log.lock().clear();

    // Selected named account stays active even while the tab's prior session is authenticated
    // (account switch / restart preparation) or agent_type was cleared (exit recovery).
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_id = Some("prev-session".into());
        tab.session_binding_nonce = Some("nonce-1".into());
        tab.reported_session_binding_nonce = Some("nonce-1".into());
    }
    let named_context = InvocationContext::new(
        "fixture".into(),
        "work".into(),
        BTreeMap::from([(
            "FIXTURE_CONFIG_DIR".into(),
            Some(named_home.to_string_lossy().into_owned()),
        )]),
        vec![named_home.clone()],
        vec![default_home, domios_root.join("provider-accounts")],
    )
    .unwrap();
    with_context(Some(named_context), || {
        run_for_cwd(&domios_root);
        run_for_cwd(&child);
        run_for_cwd(&outside);
    });
    assert_eq!(
        *log.lock(),
        vec![
            (
                domios_root.to_string_lossy().to_string(),
                Some("work".to_string()),
            ),
            (
                child.to_string_lossy().to_string(),
                Some("work".to_string()),
            ),
        ]
    );

    // Authenticated team/provisioned tab keeps provenance authorization for relaunches.
    let mut mux = fixture.service.mux.write();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.cwd = Some(outside.to_string_lossy().into_owned());
    tab.provisioned_cwd = Some(outside.to_string_lossy().into_owned());
    assert!(tab_provenance_authorized(tab));
}
