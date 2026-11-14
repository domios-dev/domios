use super::*;

fn package(name: &str, version: &str) -> Package {
    let mut package = super::super::team_catalog::bundled()[0].1.clone();
    package.document.name = name.into();
    package.document.version = version.into();
    package
}

fn channel(root: &Path, packages: &[Package]) {
    std::fs::create_dir_all(root).unwrap();
    let teams: Vec<_> = packages.iter().enumerate().map(|(index, package)| {
        let path = format!("team-{index}");
        package.write_folder(&root.join(&path)).unwrap();
        serde_json::json!({ "name": package.document.name, "version": package.document.version, "path": path, "minDomios": "0.0.0" })
    }).collect();
    std::fs::write(root.join("channel.json"), serde_json::to_vec(&serde_json::json!({
        "schemaVersion": 1, "name": "fixture", "displayName": "Fixture teams", "description": "Fixtures", "teams": teams,
    })).unwrap()).unwrap();
}

#[test]
fn index_skips_invalid_mismatched_incompatible_reserved_and_escaping_entries() {
    let root = tempfile::tempdir().unwrap();
    channel(
        root.path(),
        &[
            package("example/valid", "1.0.0"),
            package("example/mismatch", "1.0.0"),
            package("example/future", "1.0.0"),
            package("dev.domios/claim", "1.0.0"),
            package("example/invalid", "1.0.0"),
            package("example/escape", "1.0.0"),
        ],
    );
    let path = root.path().join("channel.json");
    let mut index: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    index["teams"][1]["version"] = "2.0.0".into();
    index["teams"][2]["minDomios"] = "99999.0.0".into();
    index["teams"][5]["path"] = "../outside".into();
    index["teams"].as_array_mut().unwrap().push(serde_json::json!({
        "name": "example/missing", "version": "1.0.0", "path": "team-missing", "minDomios": "0.0.0",
    }));
    std::fs::write(&path, serde_json::to_vec(&index).unwrap()).unwrap();
    std::fs::write(root.path().join("team-4/team.yaml"), b"broken: [").unwrap();
    let (_, packages, skipped) = check_index(root.path(), false, "1.20.0").unwrap();
    assert_eq!(packages.len(), 1);
    assert_eq!(packages[0].document.name, "example/valid");
    let skipped_as = |name: &str| {
        skipped
            .iter()
            .find(|entry| entry.name == name)
            .unwrap_or_else(|| panic!("{name} not skipped"))
    };
    assert_eq!(skipped.len(), 6);
    assert_eq!(
        skipped_as("example/mismatch").code,
        SkipReason::IndexMismatch
    );
    assert_eq!(
        skipped_as("example/future").code,
        SkipReason::RequiresNewerDomios
    );
    assert_eq!(
        skipped_as("example/future").required_domios.as_deref(),
        Some("99999.0.0")
    );
    assert_eq!(
        skipped_as("dev.domios/claim").code,
        SkipReason::ReservedName
    );
    assert_eq!(
        skipped_as("example/invalid").code,
        SkipReason::InvalidPackage
    );
    assert_eq!(skipped_as("example/escape").code, SkipReason::InvalidPath);
    assert_eq!(
        skipped_as("example/missing").code,
        SkipReason::MissingPackage
    );
    assert!(skipped
        .iter()
        .filter(|entry| entry.code != SkipReason::RequiresNewerDomios)
        .all(|entry| entry.required_domios.is_none()));
    index["schemaVersion"] = 2.into();
    std::fs::write(path, serde_json::to_vec(&index).unwrap()).unwrap();
    assert!(check_index(root.path(), false, "1.20.0").is_err());
}

#[test]
fn community_is_frozen_until_explicit_refresh_and_failed_refresh_keeps_cache() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("community-source");
    let official = root.path().join("official-source");
    channel(&source, &[package("example/community", "1.0.0")]);
    channel(&official, &[]);
    let store = TeamChannelStore::with_url(
        root.path().join("data"),
        official.display().to_string(),
        "1.20.0",
    );
    store
        .add(source.to_str().unwrap(), Some("community"))
        .unwrap();
    assert!(store.add(source.to_str().unwrap(), Some(OFFICIAL)).is_err());
    assert!(store
        .add(source.to_str().unwrap(), Some("../escape"))
        .is_err());
    channel(&source, &[package("example/community", "2.0.0")]);
    store.refresh_official_once();
    assert_eq!(store.packages()[0].1.document.version, "1.0.0");
    store.refresh(Some("community")).unwrap();
    assert_eq!(store.packages()[0].1.document.version, "2.0.0");
    std::fs::write(source.join("channel.json"), b"broken").unwrap();
    let rows = store.refresh(Some("community")).unwrap();
    assert_ne!(
        rows.iter()
            .find(|row| row.name == "community")
            .unwrap()
            .last_refresh
            .as_deref(),
        Some("ok")
    );
    assert_eq!(store.packages()[0].1.document.version, "2.0.0");
    assert!(store.remove(OFFICIAL).is_err());
    store.remove("community").unwrap();
    assert_eq!(store.list().unwrap().len(), 1);
}

#[test]
fn official_refresh_is_single_flight_once_per_start_and_offline_retains_cache() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    channel(&source, &[package("dev.domios/extra", "1.0.0")]);
    let store = TeamChannelStore::with_url(
        root.path().join("data"),
        source.display().to_string(),
        "1.20.0",
    );
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| store.refresh_official_once());
        }
    });
    channel(&source, &[package("dev.domios/extra", "2.0.0")]);
    store.refresh_official_once();
    assert_eq!(store.packages()[0].1.document.version, "1.0.0");
    store.refresh(Some(OFFICIAL)).unwrap();
    assert_eq!(store.packages()[0].1.document.version, "2.0.0");
    channel(&source, &[package("dev.domios/extra", "1.0.0")]);
    store.refresh(Some(OFFICIAL)).unwrap();
    assert_eq!(store.packages()[0].1.document.version, "2.0.0");
    std::fs::remove_dir_all(&source).unwrap();
    let reopened = TeamChannelStore::with_url(
        root.path().join("data"),
        source.display().to_string(),
        "1.20.0",
    );
    reopened.refresh_official_once();
    assert_eq!(reopened.packages()[0].1.document.version, "2.0.0");
    assert_ne!(
        reopened.list().unwrap()[0].last_refresh.as_deref(),
        Some("ok")
    );
}
