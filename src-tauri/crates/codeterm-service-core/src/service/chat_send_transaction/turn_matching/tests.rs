use super::*;

#[test]
fn whole_payload_containment_preserves_internal_bytes() {
    assert!(contains_written_payload(
        "earlier\n\n  alpha\r\nbeta  \nother",
        " alpha\nbeta "
    ));
    assert!(contains_written_payload(
        "<queued>alpha\rbeta</queued>",
        "alpha\nbeta"
    ));
    for record in [
        "alpha",
        "alpha beta",
        "alpha\n beta",
        "alpha\nbet",
        "alpha\nbe\u{1b}ta",
    ] {
        assert!(!contains_written_payload(record, "alpha\nbeta"));
    }
    assert!(!contains_written_payload("anything", " \r\n "));
}

#[test]
fn containment_does_not_apply_lossy_provider_normalization() {
    assert_eq!(
        normalize_written_payload(" \talpha  🚀\r\nbeta\t "),
        "alpha  🚀\nbeta"
    );
    assert!(!contains_written_payload("alpha beta", "alpha  🚀\nbeta"));
    assert!(!contains_written_payload(
        "<domios>alpha",
        "<domios>alpha</domios>"
    ));
}

#[test]
fn one_record_confirms_whole_payloads_in_send_order() {
    let candidates = [
        ("a".into(), " alpha\r\nbeta ".into(), Default::default()),
        ("b".into(), "gamma".into(), Default::default()),
        ("c".into(), "missing".into(), Default::default()),
        ("d".into(), "delta".into(), Default::default()),
    ];
    assert_eq!(
        matching_turn_send_ids(
            "prefix alpha\nbeta\n\ngamma wrapper delta suffix",
            &candidates
        ),
        ["a", "b", "d"]
    );
    assert_eq!(
        matching_turn_send_ids("delta gamma alpha\nbeta", &candidates),
        ["a"]
    );
}

#[test]
fn identical_sends_need_distinct_occurrences_within_record() {
    let candidates = [
        ("a".into(), "opaque".into(), Default::default()),
        ("b".into(), "opaque".into(), Default::default()),
    ];
    assert_eq!(
        matching_turn_send_ids("prefix opaque suffix", &candidates),
        ["a"]
    );
    assert_eq!(
        matching_turn_send_ids("prefix opaque\n\nopaque suffix", &candidates),
        ["a", "b"]
    );
}

#[test]
fn normalized_containment_requires_distinct_occurrences_in_one_normalization_scope() {
    let normalization = crate::agent::provider::TurnEvidenceNormalization {
        collapse_horizontal_whitespace: true,
        ..Default::default()
    };
    let candidates = [
        ("a".into(), "opaque\tbody".into(), normalization),
        ("b".into(), "opaque\tbody".into(), normalization),
        (
            "foreign-policy".into(),
            "opaque body".into(),
            Default::default(),
        ),
    ];
    assert_eq!(matching_turn_send_ids("opaque body", &candidates), ["a"]);
    assert_eq!(
        matching_turn_send_ids("opaque body\nopaque body", &candidates),
        ["a", "b"]
    );
    assert!(matching_turn_send_ids("opaque", &candidates).is_empty());
}

#[test]
fn glued_voice_frames_confirm_both_sends_from_one_turn() {
    let normalization = crate::agent::provider::TurnEvidenceNormalization {
        unwrap_pasted_content_wrapper: true,
        collapse_horizontal_whitespace: true,
        drop_supplementary_characters: false,
        drop_line_breaks: false,
    };
    let first = "<domios type=\"attachment\" kind=\"voice\" path=\"voice-note-a\" label=\"voice\" />\nfirst transcript";
    let second = "<domios type=\"attachment\" kind=\"voice\" path=\"voice-note-b\" label=\"voice\" />\nsecond transcript";
    let glued = format!(
        "{first}\n\n<pasted_content id=\"49dd\">\n\n\n{second}\n</pasted_content id=\"49dd\">"
    );
    let candidates = [
        ("first".into(), first.into(), normalization),
        ("second".into(), second.into(), normalization),
        ("third".into(), "unrelated transcript".into(), normalization),
    ];
    assert_eq!(
        matching_turn_send_ids(&glued, &candidates),
        ["first", "second"]
    );
    assert_eq!(matching_turn_send_ids(second, &candidates), ["second"]);
    assert_eq!(matching_turn_send_ids(first, &candidates), ["first"]);
}
