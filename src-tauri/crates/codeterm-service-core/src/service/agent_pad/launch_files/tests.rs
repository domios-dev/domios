use super::*;
use crate::service::agent_pad::{AgentPad, AgentPadRole};

#[test]
fn startup_sweep_removes_crash_handoffs_from_live_and_orphan_pads() {
    let root = tempfile::tempdir().unwrap();
    let live = AgentPad::create(root.path(), "live", AgentPadRole::Worker).unwrap();
    let orphan = AgentPad::create(root.path(), "orphan", AgentPadRole::Worker).unwrap();
    let handoffs = [
        ".runtime-launch-old.json",
        ".launch-credential-old.txt",
        "..runtime-launch-old.json.123.4.tmp",
        "..launch-credential-old.txt.123.4.tmp",
    ];
    for pad in [&live, &orphan] {
        fs::write(pad.path().join("notes.md"), b"keep notes").unwrap();
        fs::write(
            pad.path().join(".runtime-launch-notes.md"),
            b"keep other files",
        )
        .unwrap();
        for name in handoffs {
            fs::write(pad.path().join(name), b"stale").unwrap();
        }
    }
    sweep_launch_files(root.path()).unwrap();
    for pad in [&live, &orphan] {
        for name in handoffs {
            assert!(!pad.path().join(name).exists(), "{name}");
        }
        assert_eq!(
            fs::read(pad.path().join("notes.md")).unwrap(),
            b"keep notes"
        );
        assert!(pad.path().join("report.md").is_file());
        assert!(pad.path().join(".runtime-launch-notes.md").is_file());
    }
    sweep_launch_files(root.path()).unwrap();
    let empty = tempfile::tempdir().unwrap();
    sweep_launch_files(empty.path()).unwrap();
}

#[cfg(unix)]
#[test]
fn startup_sweep_unlinks_handoffs_without_following_file_or_pad_symlinks() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let pad = AgentPad::create(root.path(), "live", AgentPadRole::Worker).unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join(".runtime-launch-outside.json");
    fs::write(&target, b"outside").unwrap();
    let link = pad.path().join(".runtime-launch-link.json");
    symlink(&target, &link).unwrap();
    symlink(
        outside.path(),
        pad.path().parent().unwrap().join("foreign-pad"),
    )
    .unwrap();
    sweep_launch_files(root.path()).unwrap();
    assert!(fs::symlink_metadata(link).is_err());
    assert_eq!(fs::read(target).unwrap(), b"outside");
}

#[cfg(unix)]
#[test]
fn startup_sweep_refuses_a_symlinked_pad_root() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let pad = AgentPad::create(outside.path(), "outside", AgentPadRole::Worker).unwrap();
    let handoff = pad.path().join(".runtime-launch-outside.json");
    fs::write(&handoff, b"outside").unwrap();
    fs::create_dir(root.path().join("runtime")).unwrap();
    symlink(
        outside.path().join("runtime/agent-pads"),
        root.path().join("runtime/agent-pads"),
    )
    .unwrap();
    assert!(sweep_launch_files(root.path()).is_err());
    assert_eq!(fs::read(handoff).unwrap(), b"outside");
}

#[test]
fn startup_sweep_failure_is_reported_without_recursively_deleting_unknown_data() {
    let root = tempfile::tempdir().unwrap();
    let pad = AgentPad::create(root.path(), "live", AgentPadRole::Worker).unwrap();
    let unexpected = pad.path().join(".runtime-launch-directory.json");
    fs::create_dir(&unexpected).unwrap();
    fs::write(unexpected.join("keep.md"), b"keep").unwrap();
    assert!(sweep_launch_files(root.path()).is_err());
    assert!(unexpected.join("keep.md").is_file());
}
