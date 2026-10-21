use super::*;

#[test]
fn evidence_and_cap() {
    let one = BTreeSet::from(["task:a".into()]);
    let two = BTreeSet::from(["task:a".into(), "task:b".into()]);
    assert!(decide(&one, false, false, &[]).is_err());
    assert_eq!(decide(&two, false, false, &[]), Ok(Decision::Save));
    assert_eq!(decide(&one, true, false, &[]), Ok(Decision::Save));
    assert!(decide(&two, true, true, &[]).is_err());
}

#[test]
fn contradictions_precede_improvements() {
    let hits = [
        Relation::Covers("mem:a".into()),
        Relation::Conflict("mem:b".into()),
    ];
    assert_eq!(
        decide(&BTreeSet::new(), true, false, &hits),
        Err("conflict with mem:b".into())
    );
    assert_eq!(
        decide(&BTreeSet::new(), true, false, &hits[..1]),
        Ok(Decision::Improve("mem:a".into()))
    );
    assert_eq!(
        decide(&BTreeSet::new(), true, false, &[Relation::Unrelated]),
        Ok(Decision::Save)
    );
}

#[test]
fn confidence_requires_a_later_round_and_stale_low_is_only_a_candidate() {
    assert_eq!(confidence(None, 1, ""), "low");
    assert_eq!(confidence(Some(1), 1, "low"), "low");
    assert_eq!(confidence(Some(1), 2, "low"), "medium");
    assert_eq!(confidence(Some(2), 3, "medium"), "high");
    assert!(!dream_candidate(true, "low", 1, 60 * 86400));
    assert!(dream_candidate(true, "low", 1, 60 * 86400 + 1));
    assert!(!dream_candidate(true, "medium", 0, 90 * 86400));
    assert!(!dream_candidate(false, "low", 0, 90 * 86400));
}
