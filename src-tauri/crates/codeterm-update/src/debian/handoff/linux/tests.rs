use super::*;
use std::os::unix::{fs::PermissionsExt, net::UnixStream};

#[test]
fn launcher_directory_is_private_and_leasable_under_permissive_umask() {
    const CHILD: &str = "DOMIOS_TEST_HANDOFF_UMASK";
    if std::env::var_os(CHILD).is_none() {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "debian::handoff::linux::tests::launcher_directory_is_private_and_leasable_under_permissive_umask", "--nocapture"])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    unsafe {
        libc::umask(0);
    }
    let root = tempfile::tempdir().unwrap();
    let directory = staging::private_directory(root.path()).unwrap();
    assert_eq!(
        std::fs::metadata(directory.path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    let lease = staging::StageLease::acquire(directory.path()).unwrap();
    assert!(staging::StageLease::acquire(directory.path()).is_err());
    drop(lease);
}

#[test]
fn preflight_failure_is_persisted_and_returned_instead_of_success() {
    let root = tempfile::tempdir().unwrap();
    let directory = staging::private_directory(root.path()).unwrap();
    let staged = staging::private_directory(root.path()).unwrap();
    let plan = plan(directory.path(), staged.path(), root.path());
    let plan_path = directory.path().join("handoff.json");
    std::fs::write(&plan_path, serde_json::to_vec(&plan).unwrap()).unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o775)).unwrap();
    let listener = UnixListener::bind(directory.path().join("ready.sock")).unwrap();
    assert_eq!(execute_helper(&plan_path), 1);
    let result = acknowledge_helper(&listener, directory.path(), b"credential", 0);
    let reason = std::fs::read_to_string(directory.path().join("handoff-failure.txt")).unwrap();
    assert!(reason.contains("staged directory is not owned by this user"));
    assert_eq!(result, Err(reason));
    assert!(staged.path().join("handoff-failure.txt").is_file());
    assert!(staging::failure(root.path())
        .unwrap()
        .contains("staged directory is not owned by this user"));
}

fn plan(directory: &Path, staged: &Path, data: &Path) -> Plan {
    let ready: codeterm_types::server_bootstrap::ServerReady = serde_json::from_value(serde_json::json!({
        "protocol": 1, "api_version": 1, "build": "fixture", "instance_id": "fixture",
        "identity": {"data": data, "config": data.join("config")},
        "epoch": "fixture", "process": { "pid": 40, "created": "100", "executable": "/usr/bin/domios-app" },
        "api_address": "127.0.0.1:1234", "socket_address": "fixture", "bootstrap_address": "fixture", "resources": "/fixture"
    })).unwrap();
    Plan {
        directory: directory.into(),
        deb: staged.join("update.deb"),
        sha256: "fixture".into(),
        version: "1.18.6".into(),
        app: "/usr/bin/domios-app".into(),
        elevation: Elevation::SudoNonInteractive,
        target: QuitTarget {
            client: ready.process.clone(),
            ready: ready.clone(),
            instance_locks: vec![],
            token: String::new(),
        },
        snapshot: QuitSnapshot {
            ready,
            client: None,
        },
    }
}

#[test]
fn absent_readiness_acknowledgement_cannot_succeed() {
    let root = tempfile::tempdir().unwrap();
    let listener = UnixListener::bind(root.path().join("ready.sock")).unwrap();
    assert!(acknowledge_helper(&listener, root.path(), b"credential", 0)
        .unwrap_err()
        .contains("did not acknowledge readiness"));
}

#[test]
fn readiness_acknowledgement_releases_the_credential_only_to_the_helper() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("ready.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let helper = std::thread::spawn(move || {
        let mut stream = UnixStream::connect(path).unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(1)))
            .unwrap();
        stream.write_all(&[1]).unwrap();
        let mut length = [0; 4];
        stream.read_exact(&mut length).unwrap();
        let mut token = vec![0; u32::from_be_bytes(length) as usize];
        stream.read_exact(&mut token).unwrap();
        token
    });
    acknowledge_helper(&listener, root.path(), b"credential", 30_000).unwrap();
    assert_eq!(helper.join().unwrap(), b"credential");
}
