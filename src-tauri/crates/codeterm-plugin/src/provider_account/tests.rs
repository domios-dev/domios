use super::*;

fn context(account: &str) -> Arc<InvocationContext> {
    InvocationContext::new(
        "fixture".into(),
        account.into(),
        BTreeMap::from([
            ("ACCOUNT_FIXTURE".into(), Some(account.into())),
            ("PATH".into(), None),
        ]),
        vec![],
        vec![],
    )
    .unwrap()
}

#[test]
fn account_home_environment_removals_cover_platform_case_aliases() {
    let account = context("a");
    let mut environment = std::collections::HashMap::from([
        ("PATH".into(), "must-remove".into()),
        ("path".into(), "case-alias".into()),
        ("account_fixture".into(), "foreign".into()),
    ]);
    account.apply_environment(&mut environment);
    assert!(!environment.contains_key("PATH"));
    assert_eq!(environment["ACCOUNT_FIXTURE"], "a");
    if cfg!(windows) {
        assert!(!environment.contains_key("path"));
        assert!(!environment.contains_key("account_fixture"));
        with_context(Some(account), || {
            assert!(environment_is_selected("path"));
            assert!(super::environment("path").is_none());
            assert_eq!(super::environment("account_fixture"), Some("a".into()));
        });
    } else {
        assert_eq!(environment["path"], "case-alias");
    }
}

#[test]
fn account_home_nested_scope_restores_after_unwind_and_explicit_removal() {
    let a = context("a");
    with_context(Some(a.clone()), || {
        assert_eq!(environment("ACCOUNT_FIXTURE"), Some("a".into()));
        assert!(environment("PATH").is_none());
        assert_eq!(secret_name("fixture", "token"), None);
        assert_eq!(secret_name("foreign", "token"), None);
        let mut command = std::process::Command::new("unused-fixture-command");
        a.configure_command(&mut command);
        assert!(command
            .get_envs()
            .any(|(key, value)| key == "PATH" && value.is_none()));
        let _ = std::panic::catch_unwind(|| with_context(Some(context("b")), || panic!("fixture")));
        assert!(Arc::ptr_eq(&current().unwrap(), &a));
        with_context(None, || assert!(current().is_none()));
        assert!(Arc::ptr_eq(&current().unwrap(), &a));
    });
    assert!(current().is_none());
    assert_eq!(secret_name("fixture", "token"), Some("token".into()));
}

#[test]
fn account_home_declared_constructor_requires_a_home_and_excludes_inherited_auth() {
    let root = tempfile::tempdir().unwrap();
    let capability = storage::AccountCapability {
        credential_files: vec![],
        onboarding_flag: None,
        process_registry_directory: None,
        home_environment: "FIXTURE_HOME".into(),
        default_home: "~/.fixture".into(),
        secret_environment: vec!["FIXTURE_TOKEN".into()],
        excluded_environment: vec!["STALE_ACCOUNT_PATH".into()],
        isolated_paths: vec!["auth.json".into()],
        shared_paths: vec![],
        shared_index_paths: vec![],
        continuation: storage::AccountContinuation::Unverified,
    };
    assert!(InvocationContext::from_capability(
        "fixture".into(),
        "a".into(),
        &capability,
        BTreeMap::new(),
        vec![root.path().into()],
        vec![]
    )
    .is_err());
    let selected = InvocationContext::from_capability(
        "fixture".into(),
        "a".into(),
        &capability,
        BTreeMap::from([
            (
                "FIXTURE_HOME".into(),
                Some(root.path().to_string_lossy().into_owned()),
            ),
            ("FIXTURE_TOKEN".into(), Some("foreign-auth".into())),
        ]),
        vec![root.path().into()],
        vec![],
    )
    .unwrap();
    with_context(Some(selected), || {
        assert!(environment("FIXTURE_TOKEN").is_none());
        assert!(environment_is_selected("STALE_ACCOUNT_PATH"));
        assert!(environment("STALE_ACCOUNT_PATH").is_none());
    });
    let mut invalid = capability;
    invalid.shared_paths.push(storage::SharedStoragePath {
        path: "../foreign".into(),
        kind: storage::StoragePathKind::Directory,
        root_environment: None,
        configuration: None,
    });
    assert!(invalid.validate().is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn account_home_async_scopes_capture_blocking_workers_without_cross_task_state() {
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let run = |account: &'static str, barrier: Arc<tokio::sync::Barrier>| async move {
        scope(Some(context(account)), async move {
            barrier.wait().await;
            let worker = crate::js_runtime::spawn_blocking(move || {
                assert_eq!(environment("ACCOUNT_FIXTURE"), Some(account.into()));
                assert!(environment("PATH").is_none());
            });
            worker.await.unwrap();
            assert_eq!(environment("ACCOUNT_FIXTURE"), Some(account.into()));
        })
        .await;
        assert!(current().is_none());
    };
    let a = tokio::spawn(run("a", barrier.clone()));
    let b = tokio::spawn(run("b", barrier));
    a.await.unwrap();
    b.await.unwrap();
}

#[test]
fn account_home_catalogues_overlays_and_cache_files_are_isolated() {
    let provider = "account-catalogue-fixture";
    crate::model_catalog::register(
        provider,
        crate::model_catalog::RegisteredDefaults {
            models: vec![crate::model_catalog::model("seed", "Seed")],
            default: Some("seed".into()),
        },
    );
    let named = |name: &str| {
        InvocationContext::new(
            provider.into(),
            name.into(),
            BTreeMap::new(),
            vec![],
            vec![],
        )
        .unwrap()
    };
    let a = named("a");
    let b = named("b");
    let key_a = with_context(Some(a.clone()), || {
        crate::model_catalog::put_live(
            provider,
            vec![crate::model_catalog::model("a-only", "A")],
            Some("a-only".into()),
        );
        crate::model_catalog::put_capability_overlay(
            provider,
            "project",
            vec![crate::model_catalog::model("a-tier", "A tier")],
        );
        cache_key(provider)
    });
    with_context(Some(b), || {
        assert!(!crate::model_catalog::has_live_snapshot(provider));
        assert!(crate::model_catalog::resolve_models(provider)
            .iter()
            .all(|model| !model.id.starts_with("a-")));
        assert_ne!(cache_key(provider), key_a);
    });
    with_context(Some(a), || {
        let models = crate::model_catalog::resolve_live_models(provider).unwrap();
        assert!(models.iter().any(|model| model.id == "a-only"));
        assert!(models.iter().any(|model| model.id == "a-tier"));
        crate::model_catalog::cleanup(provider);
    });
    assert!(!crate::model_catalog::has_live_snapshot(provider));
    crate::model_catalog::cleanup(provider);
}

fn credential_context(account: &str, home: &Path) -> Arc<InvocationContext> {
    let capability: storage::AccountCapability = serde_json::from_value(serde_json::json!({
        "homeEnvironment":"FIXTURE_HOME","defaultHome":"~/.fixture","secretEnvironment":[],
        "isolatedPaths":["auth.json"],"sharedPaths":[],"continuation":"unverified"
    }))
    .unwrap();
    InvocationContext::from_capability(
        "fixture".into(),
        account.into(),
        &capability,
        BTreeMap::from([("FIXTURE_HOME".into(), Some(home.to_str().unwrap().into()))]),
        vec![home.into()],
        vec![],
    )
    .unwrap()
}

#[test]
fn account_home_secret_scope_survives_promotion_without_exposing_other_roots_or_default() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("staged-home");
    let other = root.path().join("other-home");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    let staged = credential_context("auth-stage", &home);
    let promoted = credential_context("a", &home);
    let sibling = credential_context("b", &other);
    assert_ne!(staged.cache_key(), promoted.cache_key());
    assert_eq!(
        staged.credential_secret_scope(),
        promoted.credential_secret_scope()
    );
    assert_ne!(
        staged.credential_secret_scope(),
        sibling.credential_secret_scope()
    );
    let bucket =
        codeterm_util::secret_file::plugin_secret_bucket(root.path().join("vault"), "fixture");
    let name = with_context(Some(staged.clone()), || {
        secret_name("fixture", "api-key").unwrap()
    });
    bucket.set(&name, "opaque-fixture-key").unwrap();
    with_context(Some(promoted.clone()), || {
        assert_eq!(
            bucket
                .get(&secret_name("fixture", "api-key").unwrap())
                .unwrap(),
            "opaque-fixture-key"
        )
    });
    with_context(Some(sibling.clone()), || {
        assert!(bucket
            .get(&secret_name("fixture", "api-key").unwrap())
            .is_err())
    });
    with_context(None, || {
        assert!(bucket
            .get(&secret_name("fixture", "api-key").unwrap())
            .is_err());
        assert!(secret_name("fixture", &name).is_none());
    });
    let reference = "domios:provider:fixture:key";
    let staged_ref = secret_reference("fixture", reference, Some(&staged)).unwrap();
    assert_eq!(
        Some(staged_ref.clone()),
        secret_reference("fixture", reference, Some(&promoted))
    );
    assert_ne!(
        Some(staged_ref.clone()),
        secret_reference("fixture", reference, Some(&sibling))
    );
    assert_eq!(
        secret_reference("fixture", reference, None).as_deref(),
        Some(reference)
    );
    assert!(secret_reference("fixture", &staged_ref, None).is_none());
    assert!(secret_reference(
        "fixture",
        &staged_ref.replace(":account:", ":Account:"),
        None
    )
    .is_none());
    assert!(secret_reference("foreign", reference, Some(&promoted)).is_none());
    assert!(secret_reference("fixture", reference, Some(&context("a"))).is_none());
}

#[test]
fn canonical_access_allows_protected_ancestors_without_opening_protected_roots() {
    let temp = tempfile::tempdir().unwrap();
    let domios_root = temp.path().join(".domios");
    let general = domios_root.join("general");
    let accounts_root = domios_root.join("provider-accounts");
    let own_home = accounts_root.join("fixture").join("a");
    let other_home = accounts_root.join("fixture").join("b");
    let usual = temp.path().join("usual");
    for dir in [&domios_root, &general, &own_home, &other_home, &usual] {
        std::fs::create_dir_all(dir).unwrap();
    }
    let context = InvocationContext::new(
        "fixture".into(),
        "a".into(),
        BTreeMap::from([(
            "FIXTURE_HOME".into(),
            Some(own_home.to_string_lossy().into_owned()),
        )]),
        vec![own_home.clone()],
        vec![usual.clone(), accounts_root.clone()],
    )
    .unwrap();

    assert_eq!(context.read_access(&domios_root), Some(false));
    assert_ne!(context.canonical_access(&domios_root), Some(false));
    assert_ne!(context.canonical_access(&general), Some(false));
    assert_eq!(context.canonical_access(&own_home), Some(true));
    assert_eq!(context.canonical_access(&accounts_root), Some(false));
    assert_eq!(context.canonical_access(&other_home), Some(false));
    assert_eq!(context.canonical_access(&usual), Some(false));
}

#[cfg(any(unix, windows))]
mod redirected_shared_reads;
