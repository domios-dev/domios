use super::*;

fn link(target: &Path, path: &Path, directory: bool) {
    #[cfg(unix)]
    {
        let _ = directory;
        std::os::unix::fs::symlink(target, path).unwrap();
    }
    #[cfg(windows)]
    if directory {
        let mut command = std::process::Command::new("cmd");
        codeterm_util::process_util::no_window_std(&mut command);
        let output = command
            .args(["/C", "mklink", "/J"])
            .arg(path)
            .arg(target)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    } else {
        std::os::windows::fs::symlink_file(target, path).unwrap();
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
    let exclusions = vec![root.path().join("named"), usual_private.clone()];
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
