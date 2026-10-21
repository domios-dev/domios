use super::*;

#[test]
fn job_check_refuses_only_a_job_that_kills_the_server_with_its_launcher() {
    let killed = job_membership(true, true);
    assert_eq!(killed, JobMembership::KilledWithLauncher);
    assert!(job_refusal(killed).is_some());

    let survives = job_membership(true, false);
    assert_eq!(survives, JobMembership::SurvivesLauncher);
    assert_eq!(job_refusal(survives), None);

    for kill_on_close in [false, true] {
        let outside = job_membership(false, kill_on_close);
        assert_eq!(outside, JobMembership::Outside);
        assert_eq!(job_refusal(outside), None);
    }
}

#[test]
fn only_a_refused_breakaway_retries_without_the_flag() {
    assert!(retry_without_breakaway(&io::Error::from_raw_os_error(5)));
    assert!(!retry_without_breakaway(&io::Error::from_raw_os_error(2)));
    assert!(!retry_without_breakaway(&io::Error::other("access denied")));
    assert!(!retry_without_breakaway(&io::Error::from(
        io::ErrorKind::PermissionDenied
    )));
}

#[test]
fn stderr_tail_keeps_the_last_meaningful_line_of_a_drained_pipe() {
    let mut output = b"starting\n".repeat(4096);
    output.extend_from_slice(b"domios-server refused: opaque-cause-17\n\n  \n");
    let tail = StderrTail::follow(Some(io::Cursor::new(output)));
    assert_eq!(
        tail.last_line(Duration::from_secs(5)).as_deref(),
        Some("domios-server refused: opaque-cause-17")
    );
    assert_eq!(
        StderrTail::follow(None::<io::Cursor<Vec<u8>>>).last_line(Duration::ZERO),
        None
    );
}

#[test]
fn launch_error_carries_the_child_stderr_detail() {
    let error = with_stderr_detail(
        io::Error::new(io::ErrorKind::TimedOut, "outer-failure"),
        Some("opaque-cause-17".into()),
    );
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(error.to_string(), "outer-failure: opaque-cause-17");
    let bare = with_stderr_detail(io::Error::other("outer-failure"), None);
    assert_eq!(bare.to_string(), "outer-failure");
}
