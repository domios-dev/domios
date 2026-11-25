use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;

use super::{
    authorized_cwd, domios_owned_canonical_directory, eligible_target_with_root, for_tab_with_root,
    for_tab_with_root_and_hook, is_domios_owned_directory, tab_provenance_authorized,
    PreTrustTarget,
};
use crate::agent::test_provider::MockProvider;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;
use codeterm_plugin::provider_account::{with_context, InvocationContext};

fn remove_directory_link(link: &Path) {
    #[cfg(windows)]
    std::fs::remove_dir(link).unwrap();
    #[cfg(not(windows))]
    std::fs::remove_file(link).unwrap();
}

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

#[test]
fn directory_link_inside_domios_root_pointing_outside_is_not_domios_owned() {
    let temp = tempfile::tempdir().unwrap();
    let domios_root = temp.path().join(".domios");
    let pad = domios_root.join("runtime").join("agent-pads").join("tab-1");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&pad).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let escape = pad.join("escape");
    codeterm_util::fs_links::directory_link(&outside, &escape).unwrap();

    assert!(domios_owned_canonical_directory(&escape, &domios_root).is_none());
    assert!(!is_domios_owned_directory(&escape, &domios_root));
}

#[cfg(windows)]
#[test]
fn windows_case_and_separator_variations_match_domios_root_and_reject_siblings() {
    let temp = tempfile::tempdir().unwrap();
    let domios_root = temp.path().join(".Domios");
    let child = domios_root.join("WorkSpaces").join("Fresh");
    let sibling = temp.path().join(".Domios-Other");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::create_dir_all(&sibling).unwrap();

    let canonical_child = domios_owned_canonical_directory(&child, &domios_root).unwrap();
    let varied_child = child
        .to_string_lossy()
        .to_ascii_lowercase()
        .replace('\\', "/");
    let varied_root = domios_root.to_string_lossy().to_ascii_uppercase();
    let varied_sibling = sibling.to_string_lossy().to_ascii_uppercase();

    assert_eq!(
        domios_owned_canonical_directory(Path::new(&varied_child), Path::new(&varied_root)),
        Some(canonical_child.clone())
    );
    assert_eq!(
        eligible_target_with_root(
            false,
            Some(&varied_child),
            Some(&varied_child),
            Path::new(&varied_root),
        ),
        Some(PreTrustTarget::DomiosOwned {
            raw_cwd: varied_child,
            canonical_cwd: canonical_child,
        })
    );
    assert!(
        domios_owned_canonical_directory(Path::new(&varied_sibling), Path::new(&varied_root))
            .is_none()
    );
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
    let canonical_root = domios_owned_canonical_directory(&domios_root, &domios_root).unwrap();
    let canonical_child = domios_owned_canonical_directory(&child, &domios_root).unwrap();

    assert_eq!(
        eligible_target_with_root(false, Some(root_str), Some(root_str), &domios_root),
        Some(PreTrustTarget::DomiosOwned {
            raw_cwd: root_str.to_string(),
            canonical_cwd: canonical_root,
        })
    );
    assert_eq!(
        eligible_target_with_root(false, Some(child_str), Some(child_str), &domios_root),
        Some(PreTrustTarget::DomiosOwned {
            raw_cwd: child_str.to_string(),
            canonical_cwd: canonical_child,
        })
    );
    assert_eq!(
        eligible_target_with_root(false, Some(child_str), Some(root_str), &domios_root),
        None
    );
    assert_eq!(
        eligible_target_with_root(false, Some(outside_str), Some(outside_str), &domios_root),
        None
    );
    assert_eq!(
        eligible_target_with_root(true, Some(outside_str), Some(outside_str), &domios_root),
        Some(PreTrustTarget::Provenance(outside_str.to_string()))
    );
}

#[test]
fn retargeted_domios_alias_never_grants_outside_target() {
    let temp = tempfile::tempdir().unwrap();
    let domios_root = temp.path().join(".domios");
    let pad = domios_root.join("runtime").join("agent-pads").join("tab-1");
    let inside = domios_root.join("inside-target");
    let outside = temp.path().join("outside-target");
    for dir in [&pad, &inside, &outside] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let alias = pad.join("link");
    codeterm_util::fs_links::directory_link(&inside, &alias).unwrap();

    let alias_str = alias.to_string_lossy().to_string();
    let canonical_inside = domios_owned_canonical_directory(&inside, &domios_root).unwrap();

    // Authorization binds to the canonical inside directory, never the mutable alias path.
    let approved =
        eligible_target_with_root(false, Some(&alias_str), Some(&alias_str), &domios_root)
            .expect("inside alias authorized");
    assert_eq!(approved.write_cwd(), canonical_inside);

    // Unmodified alias passes the canonical inside path (not the alias) to pre_trust_directory.
    let log = Arc::new(Mutex::new(Vec::new()));
    let (mut mux, tab_id) = MultiplexerState::new();
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("fixture".into());
        tab.cwd = Some(alias_str.clone());
    }
    let fixture = ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::new("fixture").with_pre_trust_log(log.clone()),
    ));
    for_tab_with_root(
        &fixture.service,
        &fixture.service.agent_registry,
        "fixture",
        &tab_id,
        Some(&alias_str),
        &domios_root,
    );
    assert_eq!(*log.lock(), vec![(canonical_inside.clone(), None)]);
    log.lock().clear();

    // Retargeting the alias to an outside folder between authorization and trust write
    // skips the trust write and never grants the outside directory.
    for_tab_with_root_and_hook(
        &fixture.service,
        &fixture.service.agent_registry,
        "fixture",
        &tab_id,
        Some(&alias_str),
        &domios_root,
        || {
            remove_directory_link(&alias);
            codeterm_util::fs_links::directory_link(&outside, &alias).unwrap();
        },
    );
    assert!(log.lock().is_empty());

    // Even after retargeting, the previously approved write target still names the canonical
    // inside directory, never the alias or outside folder.
    assert_eq!(approved.write_cwd(), canonical_inside);
    assert!(!approved.still_valid_for_write(&domios_root));
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
    let canonical_root = domios_owned_canonical_directory(&domios_root, &domios_root).unwrap();
    let canonical_child = domios_owned_canonical_directory(&child, &domios_root).unwrap();

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
            (canonical_root.clone(), None),
            (canonical_child.clone(), None)
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
            (canonical_root, Some("work".to_string())),
            (canonical_child, Some("work".to_string())),
        ]
    );

    // Authenticated team/provisioned tab keeps provenance authorization for relaunches.
    let mut mux = fixture.service.mux.write();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.cwd = Some(outside.to_string_lossy().into_owned());
    tab.provisioned_cwd = Some(outside.to_string_lossy().into_owned());
    assert!(tab_provenance_authorized(tab));
}
