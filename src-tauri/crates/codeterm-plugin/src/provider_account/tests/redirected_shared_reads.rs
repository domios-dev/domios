use super::*;

fn link(target: &Path, path: &Path, directory: bool) {
    if directory {
        codeterm_util::fs_links::directory_link(target, path).unwrap();
    } else {
        codeterm_util::fs_links::file_link(target, path).unwrap();
    }
}

fn unlink(path: &Path, directory: bool) {
    #[cfg(unix)]
    {
        let _ = directory;
        std::fs::remove_file(path).unwrap();
    }
    #[cfg(windows)]
    if directory {
        std::fs::remove_dir(path).unwrap();
    } else {
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn account_home_shared_targets_reject_preexisting_and_replaced_directory_and_file_links() {
    assert_hard_linked_credentials_are_excluded();
    assert_retired_credential_identity_stays_excluded();
    let root = tempfile::tempdir().unwrap();
    let own = root.path().join("named/a");
    let foreign = root.path().join("named/b");
    let usual = root.path().join("usual");
    let history = root.path().join("authorized-history");
    for path in [&own, &foreign, &usual, &history] {
        std::fs::create_dir_all(path).unwrap();
    }
    let private = foreign.join("auth.json");
    let usual_private = usual.join("config.toml");
    std::fs::write(&private, "opaque-private").unwrap();
    std::fs::write(&usual_private, "opaque-usual-config").unwrap();
    std::fs::write(history.join("record"), "own-session").unwrap();
    let exclusions = vec![
        root.path().join("named"),
        private.clone(),
        usual_private.clone(),
    ];
    let context = InvocationContext::new(
        "fixture".into(),
        "a".into(),
        BTreeMap::new(),
        vec![own.clone()],
        vec![usual.clone(), root.path().join("named")],
    )
    .unwrap()
    .with_shared_exclusions(exclusions.clone())
    .unwrap();
    for directory in [true, false] {
        let alias = usual.join(if directory {
            "sessions"
        } else {
            "history.jsonl"
        });
        let grant = || {
            if directory {
                SharedReadGrant::Directory {
                    path: alias.clone(),
                }
            } else {
                SharedReadGrant::File {
                    path: alias.clone(),
                }
            }
        };
        let secret_target = if directory { &foreign } else { &private };
        link(secret_target, &alias, directory);
        assert!(context.clone().with_shared_reads(vec![grant()]).is_err());
        unlink(&alias, directory);
        if !directory {
            link(&usual_private, &alias, false);
            assert!(context.clone().with_shared_reads(vec![grant()]).is_err());
            unlink(&alias, false);
        }
        let authorized = if directory {
            history.clone()
        } else {
            history.join("record")
        };
        link(&authorized, &alias, directory);
        let selected = context.clone().with_shared_reads(vec![grant()]).unwrap();
        let read = if directory {
            alias.join("record")
        } else {
            alias.clone()
        };
        with_context(Some(selected.clone()), || {
            assert_eq!(read_to_string(&read).unwrap(), "own-session")
        });
        unlink(&alias, directory);
        link(secret_target, &alias, directory);
        let leaked = if directory {
            alias.join("auth.json")
        } else {
            alias.clone()
        };
        assert_eq!(selected.read_access(&leaked), Some(false));
        assert_eq!(selected.read_access(&private), Some(false));
        with_context(Some(selected), || assert!(open_read(&leaked).is_err()));
        unlink(&alias, directory);
    }
}

fn assert_hard_linked_credentials_are_excluded() {
    let root = tempfile::tempdir().unwrap();
    let credential = root.path().join("auth.json");
    let shared = root.path().join("sessions");
    std::fs::create_dir(&shared).unwrap();
    std::fs::write(&credential, "opaque-private").unwrap();
    let alias = shared.join("history.jsonl");
    std::fs::hard_link(&credential, &alias).unwrap();
    let exclusions = vec![credential];
    assert!(SharedReadGrant::File { path: alias }
        .pin(&exclusions)
        .is_err());
    assert!(SharedReadGrant::Directory { path: shared }
        .pin(&exclusions)
        .is_err());
}

fn assert_retired_credential_identity_stays_excluded() {
    let root = tempfile::tempdir().unwrap();
    let own = root.path().join("own");
    std::fs::create_dir(&own).unwrap();
    let credential = root.path().join("auth.json");
    let retired = root.path().join("retired.json");
    std::fs::write(&credential, "opaque-private").unwrap();
    std::fs::hard_link(&credential, &retired).unwrap();
    let context = InvocationContext::new(
        "fixture".into(),
        "own".into(),
        BTreeMap::new(),
        vec![own],
        vec![root.path().into()],
    )
    .unwrap()
    .with_shared_exclusions(vec![credential.clone()])
    .unwrap();
    std::fs::remove_file(&credential).unwrap();
    std::fs::write(&credential, "replacement-private").unwrap();
    assert_eq!(context.read_access(&retired), Some(false));
    assert!(context
        .with_shared_reads(vec![SharedReadGrant::File { path: retired }])
        .is_err());
}
