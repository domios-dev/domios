use super::*;

#[test]
fn every_plugin_call_gets_the_standard_minute_and_transcription_outlasts_it() {
    assert_eq!(
        super::super::host_exec::wait_bound("call-deadline-fixture"),
        STANDARD
    );
    assert_eq!(LongCall::Transcription.bound(), Duration::from_secs(300));
    assert!(LongCall::Transcription.bound() > STANDARD);
    let now = Instant::now();
    let (cancellation, deadline) = LongCall::Transcription.interrupt_from(now);
    assert_eq!(deadline, now + LongCall::Transcription.bound());
    assert!(!cancellation.is_cancelled());
}
