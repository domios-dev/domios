use super::*;
use windows_sys::Win32::System::Threading::{ResumeThread, PROCESS_INFORMATION, STARTUPINFOW};

#[test]
fn confirmed_windows_process_releases_admission_before_capture_cleanup() {
    let owner = Arc::new(ProbeResourceOwner::new());
    let deadlines = Deadlines::start(Duration::from_secs(10), Duration::from_secs(10)).unwrap();
    let mut run = WindowsRun::new(deadlines, owner.reserve().unwrap(), &owner);
    run.stdout.buffer_registered = run.register(ResourceKind::OverlappedIo);
    run.stderr.buffer_registered = run.register(ResourceKind::OverlappedIo);
    run.parent_exited = true;
    run.assigned = true;
    run.job_empty = true;
    run.release_process_admission();
    assert_eq!(
        owner.snapshot().free,
        super::super::owner::MAX_RESOURCE_SLOTS
    );
    assert!(run.stdout.buffer_registered);
    assert!(run.stderr.buffer_registered);

    let mut replacement = owner.reserve().unwrap();
    replacement.register_resource().unwrap();
    run.permanent_unconfirmed = true;
    let failure = run
        .finish_failure_with(ProbeFailure::new("probe_capture_failed"))
        .unwrap_err();
    assert_eq!(failure.primary, "probe_capture_failed");
    assert_eq!(owner.snapshot().active, 1);
    replacement.resource_settled().unwrap();
    replacement.release().unwrap();
    assert_eq!(
        owner.snapshot().free,
        super::super::owner::MAX_RESOURCE_SLOTS
    );
}

#[test]
fn settled_capture_buffers_do_not_poison_owner_capacity() {
    let owner = Arc::new(ProbeResourceOwner::new());
    let deadlines = Deadlines::start(Duration::from_secs(10), Duration::from_secs(10)).unwrap();
    let mut run = WindowsRun::new(deadlines, owner.reserve().unwrap(), &owner);
    run.stdout.buffer_registered = run.register(ResourceKind::OverlappedIo);
    run.stderr.buffer_registered = run.register(ResourceKind::OverlappedIo);
    run.stdout.connect_buffer_registered = run.register(ResourceKind::OverlappedIo);
    run.stderr.connect_buffer_registered = run.register(ResourceKind::OverlappedIo);
    run.permanent_unconfirmed = true;
    run.unknown_completion = true;

    let failure = run
        .finish_failure_with(ProbeFailure::new("probe_capture_failed"))
        .unwrap_err();
    assert_eq!(failure.primary, "probe_capture_failed");
    assert!(failure.is_unconfirmed());
    assert_eq!(
        owner.snapshot().free,
        super::super::owner::MAX_RESOURCE_SLOTS
    );
    assert_eq!(owner.snapshot().unconfirmed, 0);
}

pub fn verify_empty_completion_port_timeout() {
    let owner = Arc::new(ProbeResourceOwner::new());
    let deadlines = Deadlines::start(Duration::from_secs(10), Duration::from_secs(10)).unwrap();
    let mut run = WindowsRun::new(deadlines, owner.reserve().unwrap(), &owner);
    let port = unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, null_mut(), 0, 1) };
    run.port = OwnedHandle::new(port).unwrap();
    run.port_registered = run.register(ResourceKind::OverlappedIo);
    assert!(run.port_registered);

    assert!(!run.wait_event(Some(Duration::ZERO)).unwrap());
    assert!(run.port.is_open());
    assert!(run.port_registered);
    assert!(!run.permanent_unconfirmed);
    assert!(!run.unknown_completion);
    assert!(run.failure.is_none());
    assert_eq!(owner.snapshot().active, 1);

    let failure = run.finish_failure().unwrap_err();
    assert!(!failure.is_unconfirmed(), "{failure:?}");
    assert!(failure.cleanup_errors.is_empty(), "{failure:?}");
    assert_eq!(
        owner.snapshot().free,
        super::super::owner::MAX_RESOURCE_SLOTS
    );
    assert_eq!(owner.snapshot().unconfirmed, 0);
}

pub fn verify_early_exit_notification() {
    let owner = Arc::new(ProbeResourceOwner::new());
    let mut run = suspended_job_process(&owner);

    run.observe_job_event(JOB_OBJECT_MSG_EXIT_PROCESS, run.child_id)
        .unwrap();
    assert!(run.parent_exit_pending);
    assert!(!run.parent_exited);
    assert!(!run.permanent_unconfirmed);
    assert!(!run.wait_event(Some(Duration::ZERO)).unwrap());
    assert!(run.process.is_open());
    assert!(run.process_registered);
    assert!(run.parent_exit_pending);
    assert!(!run.parent_exited);
    assert!(!run.permanent_unconfirmed);

    assert_ne!(
        unsafe { ResumeThread(run.primary_thread.raw().unwrap()) },
        u32::MAX
    );
    assert!(run
        .wait_event(Some(run.deadlines.remaining_operation()))
        .unwrap());
    assert!(run.parent_exited);
    assert!(!run.parent_exit_pending);
    assert_eq!(run.exit_code, Some(7));
    assert!(!run.process.is_open());
    assert!(!run.process_registered);
    assert!(!run.primary_thread.is_open());
    assert!(!run.permanent_unconfirmed);
    run.observe_job_event(JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO, 0)
        .unwrap();
    assert!(run.failure_resources_settled());
    run.release_reservation().unwrap();
    assert_eq!(
        owner.snapshot().free,
        super::super::owner::MAX_RESOURCE_SLOTS
    );
}

#[test]
fn queued_job_notifications_after_confirmed_close_preserve_capture() {
    let owner = Arc::new(ProbeResourceOwner::new());
    let deadlines = Deadlines::start(Duration::from_secs(10), Duration::from_secs(10)).unwrap();
    let mut run = WindowsRun::new(deadlines, owner.reserve().unwrap(), &owner);
    run.prepare_containment().unwrap();
    run.assigned = true;
    run.parent_exited = true;
    run.job_empty = true;
    run.stdout.buffer_registered = run.register(ResourceKind::OverlappedIo);
    run.stdout.pending_active = true;
    run.close_completed_runtime_handles().unwrap();
    assert!(!run.job.is_open());
    assert!(run.port.is_open());
    for message in [
        JOB_OBJECT_MSG_EXIT_PROCESS,
        JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO,
    ] {
        run.observe_job_event(message, run.child_id).unwrap();
        assert!(run.stdout.pending_active);
        assert!(run.port.is_open());
        assert!(!run.permanent_unconfirmed);
    }
    run.stdout.pending_active = false;
    run.settle_idle_buffers();
    run.close_completed_runtime_handles().unwrap();
    assert!(run.failure_resources_settled());
    run.release_reservation().unwrap();
    assert_eq!(owner.snapshot().active, 0);
}

fn suspended_job_process(owner: &Arc<ProbeResourceOwner>) -> WindowsRun {
    let deadlines = Deadlines::start(Duration::from_secs(10), Duration::from_secs(10)).unwrap();
    let mut run = WindowsRun::new(deadlines, owner.reserve().unwrap(), owner);
    run.prepare_containment().unwrap();
    let mut command = capture::power_shell_command_line("exit 7");
    let mut startup = STARTUPINFOW::default();
    startup.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    let mut information = PROCESS_INFORMATION::default();
    assert_ne!(
        unsafe {
            CreateProcessW(
                null(),
                command.as_mut_ptr(),
                null(),
                null(),
                0,
                CREATE_SUSPENDED | CREATE_NO_WINDOW,
                null(),
                null(),
                &startup,
                &mut information,
            )
        },
        0
    );
    run.process = OwnedHandle::new(information.hProcess).unwrap();
    run.primary_thread = OwnedHandle::new(information.hThread).unwrap();
    run.process_registered = run.register(ResourceKind::Child);
    run.thread_registered = run.register(ResourceKind::Child);
    run.child_id = information.dwProcessId;
    assert_ne!(
        unsafe { AssignProcessToJobObject(run.job.raw().unwrap(), run.process.raw().unwrap()) },
        0
    );
    run.assigned = true;
    run.job_empty = false;

    run
}

#[test]
fn signaled_parent_confirms_job_after_early_exit_notice_within_deadline() {
    for capture_pending in [false, true] {
        let owner = Arc::new(ProbeResourceOwner::new());
        let mut run = suspended_job_process(&owner);
        run.stderr.eof = true;
        run.stdout.eof = !capture_pending;
        run.stdout.pending_active = capture_pending;
        run.observe_job_event(JOB_OBJECT_MSG_EXIT_PROCESS, run.child_id)
            .unwrap();
        assert!(run.parent_exit_pending);
        assert!(!run.job_empty);
        assert!(!run.parent_exited);
        assert_ne!(
            unsafe { ResumeThread(run.primary_thread.raw().unwrap()) },
            u32::MAX
        );
        assert!(run
            .wait_event(Some(run.deadlines.remaining_operation()))
            .unwrap());
        assert!(run.parent_exited);
        assert_eq!(run.exit_code, Some(7));
        assert!(!run.parent_exit_pending);
        assert!(!run.process.is_open());
        assert!(!run.process_registered);
        assert!(!run.primary_thread.is_open());
        assert!(!run.thread_registered);
        // Process signaling may precede Job accounting reaching zero under load.
        while !run.job_empty {
            assert!(run.job.is_open());
            assert!(run.port.is_open());
            assert!(!run.runtime_success_ready());
            let remaining = run.deadlines.remaining_operation();
            assert!(
                !remaining.is_zero(),
                "Job did not drain within the operation deadline"
            );
            assert!(
                run.wait_event(Some(remaining)).unwrap(),
                "Job completion wait reached the operation deadline"
            );
        }
        assert!(!run.job.is_open());
        assert_eq!(run.port.is_open(), capture_pending);
        assert_eq!(run.runtime_success_ready(), !capture_pending);
        if capture_pending {
            let overlapped = &mut run.stdout.pending.overlapped as *mut OVERLAPPED;
            run.observe_read_event(true, overlapped, 0, Some(ERROR_BROKEN_PIPE))
                .unwrap();
            run.close_completed_runtime_handles().unwrap();
        }
        assert!(run.runtime_success_ready());
        assert!(run.failure_resources_settled());
        assert!(!run.permanent_unconfirmed);
        run.release_reservation().unwrap();
        assert_eq!(owner.snapshot().active, 0);
    }
}
