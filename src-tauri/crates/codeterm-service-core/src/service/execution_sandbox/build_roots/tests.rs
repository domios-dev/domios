use super::*;

fn base() -> PathBuf {
    std::env::current_dir().unwrap()
}

#[test]
fn absent_environment_selects_only_declared_defaults_on_each_platform() {
    let base = base();
    for platform in ["windows", "macos", "linux"] {
        let selected = select(
            &base.join("lane"),
            &base.join("home"),
            platform,
            &BTreeMap::new(),
        );
        assert_eq!(selected.len(), 5);
        assert_eq!(selected["CARGO_HOME"], base.join("home/.cargo"));
        assert_eq!(
            selected["CARGO_BUILD_BUILD_DIR"],
            base.join("home/.cargo/codeterm-lanes")
        );
        assert_eq!(
            selected["npm_config_store_dir"],
            base.join("lane/.codeterm-pnpm-store")
        );
        assert_eq!(selected["TMPDIR"], base.join("lane/.codeterm-tmp"));
        assert_eq!(
            selected["SCCACHE_DIR"],
            base.join(match platform {
                "windows" => "home/AppData/Local/Mozilla/sccache/cache",
                "macos" => "home/Library/Caches/Mozilla.sccache",
                _ => "home/.cache/sccache",
            })
        );
    }
}

#[test]
fn explicit_environment_is_bounded_to_declared_roots() {
    let base = base();
    let env = BTreeMap::from([
        (
            "CARGO_HOME".into(),
            base.join("cargo").display().to_string(),
        ),
        (
            "CODETERM_RUST_CACHE_ROOT".into(),
            base.join("rust").display().to_string(),
        ),
        (
            "SCCACHE_DIR".into(),
            base.join("sccache").display().to_string(),
        ),
        (
            "UNDECLARED_ROOT".into(),
            base.join("foreign").display().to_string(),
        ),
        (
            "TMPDIR".into(),
            base.join("foreign-temp").display().to_string(),
        ),
    ]);
    let selected = select(&base, &base, "linux", &env);
    assert_eq!(selected["CARGO_HOME"], base.join("cargo"));
    assert_eq!(selected["CARGO_BUILD_BUILD_DIR"], base.join("rust/lanes"));
    assert_eq!(
        selected["npm_config_store_dir"],
        base.join(".codeterm-pnpm-store")
    );
    let expected: Vec<_> = selected
        .values()
        .map(|path| format!("canonical:{}", path.display()))
        .collect();
    let resolved = resolve(selected, |path| Ok(format!("canonical:{}", path.display()))).unwrap();
    assert_eq!(resolved.writable_roots, expected);
    assert_eq!(resolved.environment["TMPDIR"], resolved.environment["TEMP"]);
    assert_eq!(resolved.environment["TMPDIR"], resolved.environment["TMP"]);
    assert!(!resolved.environment.contains_key("UNDECLARED_ROOT"));
    assert!(resolved
        .writable_roots
        .iter()
        .all(|root| !root.contains("foreign")));
}

#[test]
fn relative_roots_and_failed_canonicalization_refuse_the_policy() {
    assert!(resolve(
        BTreeMap::from([("CARGO_HOME".into(), PathBuf::from("relative"))]),
        |_| panic!("must refuse before IO")
    )
    .is_err());
    assert!(
        resolve(BTreeMap::from([("CARGO_HOME".into(), base())]), |_| Err(
            "canonicalization refused".into()
        ))
        .is_err()
    );
}

#[test]
fn explicit_store_wins_and_empty_environment_uses_defaults() {
    let base = base();
    let empty = BTreeMap::from([
        ("CARGO_HOME".into(), String::new()),
        ("SCCACHE_DIR".into(), String::new()),
    ]);
    assert_eq!(
        select(&base, &base, "linux", &empty),
        select(&base, &base, "linux", &BTreeMap::new())
    );
    let configured = BTreeMap::from([(
        "npm_config_store_dir".into(),
        base.join("configured-store").display().to_string(),
    )]);
    let selected = select(&base, &base, "linux", &configured);
    assert_eq!(
        selected["npm_config_store_dir"],
        base.join("configured-store")
    );
}
