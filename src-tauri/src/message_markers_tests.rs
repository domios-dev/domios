use super::*;

/// Cross-language parity fixture — mirror these exact pairs in
/// `packages/chat-core/src/markers.test.ts`. If either side drifts, its test
/// fails.
const PARITY: &[(&str, &str)] = &[
    (
        "-=-codeterm:voice file=/u/v.webm-=-привет",
        "voice→transcript payload",
    ),
    (
        "-=-codeterm:voice file=/u/v.webm status=untranscribed-=-no daemon",
        "voice→untranscribed note in payload",
    ),
    ("-=-codeterm:photo file=/u/p.jpg name=p.jpg-=-", "photo"),
    (
        "-=-codeterm:telegram user=100000001 msg=12345-=-do the task",
        "telegram",
    ),
    ("-=-codeterm:mesh peer=peer-a pane=abc12345-=-hi", "mesh"),
    (
        "-=-codeterm:pane label=General%20Main pane=bda0e267-=-ping",
        "pane label has space",
    ),
    (
        "-=-codeterm:plugin_config plugin=local-shell name=LM%20Studio-=-hello body",
        "plugin_config (name has space)",
    ),
];

#[test]
fn convenience_constructors_match_parity_wire() {
    assert_eq!(voice("/u/v.webm", Some("привет"), None), PARITY[0].0);
    assert_eq!(voice("/u/v.webm", None, Some("no daemon")), PARITY[1].0);
    assert_eq!(photo("/u/p.jpg", Some("p.jpg")), PARITY[2].0);
    assert_eq!(
        telegram("100000001", Some("12345"), "do the task"),
        PARITY[3].0
    );
    assert_eq!(mesh("peer-a", "abc12345", "hi"), PARITY[4].0);
    assert_eq!(pane("General Main", "bda0e267", "ping"), PARITY[5].0);
    assert_eq!(
        plugin_config("local-shell", "LM Studio", "hello body"),
        PARITY[6].0
    );
}

#[test]
fn every_parity_wire_parses() {
    for (wire, label) in PARITY {
        let parsed = parse(wire).unwrap_or_else(|| panic!("parse failed: {label}"));
        assert!(!parsed.kind.is_empty(), "{label}");
    }
}

#[test]
fn parse_extracts_kind_attrs_payload() {
    let p = parse("-=-codeterm:telegram user=100000001 msg=12345-=-do the task").unwrap();
    assert_eq!(p.kind, "telegram");
    assert_eq!(p.attr("user"), Some("100000001"));
    assert_eq!(p.attr("msg"), Some("12345"));
    assert_eq!(p.payload, "do the task");
}

#[test]
fn percent_encoding_round_trips_structural_chars() {
    let nasty = "a b c=d%e\"f\ng";
    let wire = Marker::new("pane").attr("label", nasty).build();
    // No raw structural chars leaked into the frame head.
    let head = &wire[OPEN.len()..wire[OPEN.len()..].find(CLOSE).unwrap() + OPEN.len()];
    assert!(!head.contains("]]"));
    let p = parse(&wire).unwrap();
    assert_eq!(p.attr("label"), Some(nasty));
}

#[test]
fn payload_may_contain_double_brackets() {
    let wire = voice("/u/v.webm", Some("see [[x]] there"), None);
    let p = parse(&wire).unwrap();
    assert_eq!(p.kind, "voice");
    assert_eq!(p.payload, "see [[x]] there");
}

#[test]
fn parse_none_on_plain_text() {
    assert!(parse("just a normal message").is_none());
    assert!(parse("[voice] not our format").is_none());
    assert!(parse("[[ct:pane label=Main pane=abc12345]]hello").is_none());
    assert!(parse("-=-codeterm:-=-payload").is_none());
}

#[test]
fn voice_note_goes_to_payload_with_status() {
    let wire = voice("/u/v.webm", None, Some("no transcriber daemon"));
    let p = parse(&wire).unwrap();
    assert_eq!(p.attr("status"), Some("untranscribed"));
    assert_eq!(p.payload, "no transcriber daemon");
}
