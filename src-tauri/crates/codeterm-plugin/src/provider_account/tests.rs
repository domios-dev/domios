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
        assert_eq!(
            secret_name("fixture", "token"),
            Some("account:a:token".into())
        );
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
