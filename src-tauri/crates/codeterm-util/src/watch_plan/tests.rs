use super::*;

fn create(root: &Path, names: &[&str]) {
    for name in names {
        std::fs::create_dir_all(root.join(name)).unwrap();
    }
}

#[test]
fn own_gitignore_controls_nested_subscriptions() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::write(
        root.join(".gitignore"),
        "target/\nnode_modules/\ncache-*/\n!cache-kept/\n",
    )
    .unwrap();
    create(
        root,
        &[
            "src-tauri/target/debug",
            "src-tauri/crates/core",
            "packages/x/node_modules/dependency",
            "packages/x/src",
            "src",
            "cache-one",
            "cache-kept",
        ],
    );
    let planned = covering_set(root).unwrap();
    assert_eq!(
        planned,
        vec![
            DirectoryWatch {
                path: root.into(),
                recursive: false
            },
            DirectoryWatch {
                path: root.join("cache-kept"),
                recursive: true
            },
            DirectoryWatch {
                path: root.join("packages"),
                recursive: false
            },
            DirectoryWatch {
                path: root.join("packages/x"),
                recursive: false
            },
            DirectoryWatch {
                path: root.join("packages/x/src"),
                recursive: true
            },
            DirectoryWatch {
                path: root.join("src"),
                recursive: true
            },
            DirectoryWatch {
                path: root.join("src-tauri"),
                recursive: false
            },
            DirectoryWatch {
                path: root.join("src-tauri/crates"),
                recursive: true
            },
        ]
    );
    assert!(covering_set_with_limit(root, 2)
        .unwrap_err()
        .contains("subscription limit exceeded"));
}

#[test]
fn nested_gitignore_and_negation_use_directory_local_precedence() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    create(root, &["project/cache", "project/source", "other/source"]);
    std::fs::write(root.join(".gitignore"), "cache/\n").unwrap();
    std::fs::write(root.join("project/.gitignore"), "!cache/\nsource/\n").unwrap();
    let planned = covering_set(root).unwrap();
    assert!(planned.contains(&DirectoryWatch {
        path: root.join("project/cache"),
        recursive: true
    }));
    assert!(!planned
        .iter()
        .any(|watch| watch.path == root.join("project/source")));
    assert!(planned.contains(&DirectoryWatch {
        path: root.join("other"),
        recursive: true
    }));
}

#[test]
fn no_ignored_descendants_produces_one_recursive_subscription() {
    let directory = tempfile::tempdir().unwrap();
    create(directory.path(), &["src/deep", "packages/x"]);
    assert_eq!(
        covering_set(directory.path()).unwrap(),
        vec![DirectoryWatch {
            path: directory.path().into(),
            recursive: true
        }]
    );
}

#[cfg(unix)]
#[test]
fn symlinked_trees_are_never_covered_by_recursive_subscriptions() {
    let directory = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    create(directory.path(), &["source/safe"]);
    std::os::unix::fs::symlink(external.path(), directory.path().join("source/linked")).unwrap();
    let planned = covering_set(directory.path()).unwrap();
    assert_eq!(
        planned,
        vec![
            DirectoryWatch {
                path: directory.path().into(),
                recursive: false
            },
            DirectoryWatch {
                path: directory.path().join("source"),
                recursive: false
            },
            DirectoryWatch {
                path: directory.path().join("source/safe"),
                recursive: true
            },
        ]
    );
}
