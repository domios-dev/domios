use super::*;

#[test]
fn delayed_create_parses() {
    let parsed = DelayedOnly::try_parse_from(["delayed", "create", "--delay", "5m", "hello"])
        .expect("delayed create parses");
    assert!(matches!(
        parsed.action,
        DelayedAction::Create {
            delay: Some(ref v),
            ..
        } if v == "5m"
    ));
}

#[test]
fn send_every_parses_until_and_count_exclusively() {
    let parsed = SendOnly::try_parse_from(["send", "--every", "10m", "--count", "3", "hello"])
        .expect("send --every --count parses");
    assert_eq!(parsed.every.as_deref(), Some("10m"));
    assert_eq!(parsed.count, Some(3));
    assert!(SendOnly::try_parse_from([
        "send",
        "--every",
        "10m",
        "--until",
        "2026-06-14T13:00:00Z",
        "--count",
        "3",
        "hello",
    ])
    .is_err());
}

#[test]
fn delayed_create_and_edit_parse_recurring_options() {
    let parsed = DelayedOnly::try_parse_from([
        "delayed", "create", "--every", "10m", "--count", "3", "hello",
    ])
    .expect("delayed create recurrence parses");
    assert!(matches!(
        parsed.action,
        DelayedAction::Create {
            every: Some(ref every),
            count: Some(3),
            ..
        } if every == "10m"
    ));
    let parsed = DelayedOnly::try_parse_from([
        "delayed",
        "edit",
        "dmsg-1",
        "--every",
        "30m",
        "--until",
        "2026-06-14T13:00:00Z",
    ])
    .expect("delayed edit recurrence parses");
    assert!(matches!(
        parsed.action,
        DelayedAction::Edit {
            every: Some(ref every),
            until: Some(ref until),
            ..
        } if every == "30m" && until == "2026-06-14T13:00:00Z"
    ));
}
