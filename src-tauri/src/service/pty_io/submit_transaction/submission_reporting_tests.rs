use super::*;

#[test]
fn messenger_report_planner_send_share_submit_transaction() {
    for entry in [
        PtySubmitEntry::Messenger,
        PtySubmitEntry::Report,
        PtySubmitEntry::Planner,
        PtySubmitEntry::Direct,
    ] {
        let mut submit_calls = 0;
        let result = submit_entry(entry, "shared body", |text| {
            submit_calls += 1;
            assert_eq!(text, "shared body");
            Err::<(), _>(InjectionError::SubmitNeverConfirmed {
                waited: Duration::ZERO,
                screen_tail: "> shared body".into(),
            })
        });
        assert!(matches!(
            result,
            Err(InjectionError::SubmitNeverConfirmed { .. })
        ));
        assert_eq!(submit_calls, 1);
    }
}

#[test]
fn recovery_reporting_admits_a_successful_recovery() {
    let recovered = InjectionResult {
        bytes_written: 12,
        echo_observed: true,
        submit_observed: true,
        submit_anomaly: false,
        submit_key_writes: 2,
        elapsed: Duration::from_millis(1),
    };
    assert!(recovery_attempted(&recovered));
    assert!(recovered.submit_observed);
}

#[test]
fn recovery_reporting_admits_a_failed_recovery() {
    let failed = InjectionResult {
        bytes_written: 12,
        echo_observed: true,
        submit_observed: false,
        submit_anomaly: true,
        submit_key_writes: 3,
        elapsed: Duration::from_millis(1),
    };
    assert!(recovery_attempted(&failed));
    assert!(!failed.submit_observed);
}

#[test]
fn recovery_reporting_ignores_a_submit_that_needed_no_recovery() {
    for key_writes in [0usize, 1] {
        let clean = InjectionResult {
            bytes_written: 12,
            echo_observed: true,
            submit_observed: true,
            submit_anomaly: false,
            submit_key_writes: key_writes,
            elapsed: Duration::from_millis(1),
        };
        assert!(!recovery_attempted(&clean), "key_writes={key_writes}");
    }
}
