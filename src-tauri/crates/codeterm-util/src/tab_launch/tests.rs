use super::*;
use std::collections::HashMap;

fn file(root: &Path) -> std::path::PathBuf {
    let path = root.join("launch.json");
    write_new(
        &path,
        &serde_json::to_vec(&LaunchCredential {
            tab_id: "own-tab".into(),
            binding_nonce: "own-nonce".into(),
            token: "own-tab-token".into(),
        })
        .unwrap(),
    )
    .unwrap();
    path
}

#[test]
fn environment_precedes_file_and_scrubbed_children_keep_the_tab_credential() {
    let root = tempfile::tempdir().unwrap();
    let path = file(root.path());
    assert!(crate::private_file::owner_only(&path).unwrap());
    let mut env = HashMap::from([
        ("CODETERM_TAB_ID", "own-tab".to_owned()),
        ("CODETERM_SESSION_BINDING_NONCE", "own-nonce".to_owned()),
        (FILE_ENV, path.to_str().unwrap().to_owned()),
        ("CODETERM_API_TOKEN", "explicit-token".to_owned()),
    ]);
    assert_eq!(
        lookup(|key| env.get(key).cloned()).unwrap().as_deref(),
        Some("explicit-token")
    );
    env.retain(|key, _| {
        !["KEY", "PASSWORD", "SECRET", "TOKEN"]
            .iter()
            .any(|word| key.contains(*word))
    });
    assert_eq!(
        lookup(|key| env.get(key).cloned()).unwrap().as_deref(),
        Some("own-tab-token")
    );
    assert_eq!(
        resolve(
            Some("explicit-token"),
            Some(&root.path().join("missing")),
            None,
            None
        )
        .unwrap(),
        "explicit-token"
    );
}

#[test]
fn stale_nonce_foreign_tab_and_missing_identity_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let path = file(root.path());
    for (tab, nonce) in [
        (Some("own-tab"), Some("old-nonce")),
        (Some("foreign-tab"), Some("own-nonce")),
        (None, Some("own-nonce")),
        (Some("own-tab"), None),
    ] {
        assert!(resolve(None, Some(&path), tab, nonce)
            .unwrap_err()
            .contains("restart the tab"));
    }
}

#[test]
fn the_runtime_agent_marker_survives_a_harness_credential_scrub_and_grants_nothing() {
    for shape in ["KEY", "PASSWORD", "SECRET", "TOKEN"] {
        assert!(!RUNTIME_AGENT_ENV.contains(shape), "{shape}");
    }
    assert!(
        !MARKERS.contains(&RUNTIME_AGENT_ENV),
        "credential lookup never reads this hint"
    );
}

#[test]
fn every_remaining_launch_marker_refuses_without_authority() {
    for marker in MARKERS {
        for value in ["", "remaining-marker"] {
            let error = lookup(|key| (key == *marker).then(|| value.to_owned())).unwrap_err();
            assert!(error.contains("restart the tab"), "{marker}");
        }
    }
    assert_eq!(lookup(|_| None).unwrap(), None);
}

#[test]
fn missing_corrupt_and_oversized_files_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("launch.json");
    assert!(resolve(None, Some(&path), Some("own-tab"), Some("own-nonce")).is_err());
    for bytes in [b"broken".to_vec(), vec![b'x'; 8193]] {
        crate::private_file::write_atomic(&path, &bytes).unwrap();
        assert!(resolve(None, Some(&path), Some("own-tab"), Some("own-nonce")).is_err());
    }
}

#[cfg(unix)]
#[test]
fn public_files_and_symlinks_are_refused() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let root = tempfile::tempdir().unwrap();
    let path = file(root.path());
    let link = root.path().join("link.json");
    symlink(&path, &link).unwrap();
    assert!(resolve(None, Some(&link), Some("own-tab"), Some("own-nonce")).is_err());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(resolve(None, Some(&path), Some("own-tab"), Some("own-nonce")).is_err());
}
