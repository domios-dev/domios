use super::*;
#[test]
fn blueprint_journal_markers_and_outcomes_round_trip_without_string_heuristics() {
    for marker in [
        JournalMarker::Step(0),
        JournalMarker::Step(63),
        JournalMarker::Step(70000),
        JournalMarker::Step(u32::MAX),
        JournalMarker::Result,
        JournalMarker::Run,
    ] {
        let name = marker.name("step");
        assert_eq!(
            JournalMarker::from_columns(marker.index(), name).unwrap(),
            marker
        );
        assert_eq!(
            serde_json::from_value::<JournalMarker>(serde_json::to_value(marker).unwrap()).unwrap(),
            marker
        );
    }
    for outcome in [
        JournalOutcome::Done,
        JournalOutcome::Failed,
        JournalOutcome::Held,
        JournalOutcome::HandlerResult,
        JournalOutcome::Skip("skip:branch".into()),
        JournalOutcome::Bucket("ready".into()),
    ] {
        assert_eq!(
            serde_json::from_value::<JournalOutcome>(serde_json::to_value(&outcome).unwrap())
                .unwrap(),
            outcome
        );
    }
    let reason = JournalReason::UncertainEffect;
    assert_eq!(
        serde_json::from_value::<JournalReason>(serde_json::to_value(&reason).unwrap()).unwrap(),
        reason
    );
    assert!(JournalMarker::from_columns(-1, "step").is_err());
    assert!(JournalMarker::from_columns(i64::from(u32::MAX) + 1, "step").is_err());
    assert!(JournalState::parse("invalid").is_err());
}
