//! Classification controls prove retired tokens cannot dispatch destructive cleanup.
//! They do not simulate real PGID reuse, visibility hiding, or native termination.
//! Callback dispatch counts and scoped observation variants are the private result oracles.

#[cfg(unix)]
use super::*;

#[cfg(unix)]
#[test]
fn provider_update_process_retired_or_lost_token_cannot_signal() {
    assert!(termination_target(LeaderOwnership::Retired(42), 42).is_err());
    assert!(termination_target(LeaderOwnership::Lost, 42).is_err());
    assert!(termination_target(LeaderOwnership::Unspawned, 42).is_err());
    assert!(termination_target(LeaderOwnership::Retained(42), 43).is_err());
    assert!(termination_target(LeaderOwnership::Retained(1), 1).is_err());
    assert!(termination_target(LeaderOwnership::Retained(u32::MAX), u32::MAX).is_err());
    assert_eq!(
        termination_target(LeaderOwnership::Retained(42), 42).unwrap(),
        42
    );
}

#[cfg(unix)]
#[test]
fn provider_update_process_only_scoped_esrch_is_observed_absence() {
    assert_eq!(
        classify_group_observation(-1, Some(libc::ESRCH)),
        BoundaryCompletion::VisibleGroupAbsent
    );
    for errno in [
        None,
        Some(libc::EPERM),
        Some(libc::EACCES),
        Some(libc::EINTR),
    ] {
        assert_eq!(
            classify_group_observation(-1, errno),
            BoundaryCompletion::PresentOrUnknown
        );
    }
    assert_eq!(
        classify_group_observation(0, Some(libc::ESRCH)),
        BoundaryCompletion::PresentOrUnknown
    );
}

#[test]
fn provider_update_process_real_normal_children_complete() {
    let mut fixture = super::super::process_fixture::Fixture::start("success");
    fixture.release("descendant");
    fixture.release("parent");
    assert!(fixture.finish().is_ok());
}

#[cfg(unix)]
#[test]
fn provider_update_process_retired_absent_or_reused_group_has_no_signal_authority() {
    let dispatched = std::cell::Cell::new(0);
    for observation in [
        classify_group_observation(-1, Some(libc::ESRCH)),
        classify_group_observation(0, None),
    ] {
        let result = terminate_owned_group(LeaderOwnership::Retired(42), 42, |_| {
            dispatched.set(dispatched.get() + 1);
            Ok(())
        });
        assert!(result.is_err());
        assert_eq!(dispatched.get(), 0);
        if observation.confirmed() {
            assert_eq!(observation, BoundaryCompletion::VisibleGroupAbsent);
        }
    }
    let error = terminate_owned_group(LeaderOwnership::Retained(42), 42, |_| {
        Err(std::io::Error::from_raw_os_error(libc::EPERM))
    })
    .unwrap_err();
    assert_eq!(
        error,
        std::io::Error::from_raw_os_error(libc::EPERM).to_string()
    );
}

#[cfg(all(windows, debug_assertions))]
#[test]
fn provider_update_resume_failure_is_an_error_and_owned_child_is_reaped() {
    use std::os::windows::io::AsRawHandle;
    use std::process::{Command, Stdio};
    let mut command = Command::new("cmd.exe");
    command
        .args(["/d", "/c", "exit 0"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut tree = crate::provider_update::ProcessTree::prepare(&mut command).unwrap();
    let mut child = command.spawn().unwrap();
    tree.record_spawn(&child).unwrap();
    let evidence = codeterm_util::process_util::diagnostics::snapshot(&child);
    let threads = evidence["threads"]["entries"].as_array().unwrap();
    assert_eq!(threads.len(), 1, "{evidence}");
    assert_eq!(threads[0]["suspend_count"], 1, "{evidence}");
    let error = tree
        .attach_with_resume(&child, |handle| {
            assert_eq!(handle, child.as_raw_handle().cast());
            0xC0000022u32 as i32
        })
        .unwrap_err();
    assert!(error.contains("NTSTATUS 0xc0000022"), "{error}");
    assert!(tree.assigned);
    assert!(child.try_wait().unwrap().is_none());
    tree.terminate(&mut child).unwrap();
    assert_eq!(child.wait().unwrap().code(), Some(1));
    tree.reaped(&child);
    assert!(tree
        .wait_completion(std::time::Instant::now() + std::time::Duration::from_secs(5))
        .unwrap()
        .confirmed());
}
