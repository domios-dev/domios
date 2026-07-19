use super::*;
use crate::usage_parser::{UsageParseError, UsageParser};

const FIXTURE: &str = include_str!("../fixtures/usage_v2_1.txt");

#[test]
fn parses_fixture_v2_1() {
    let parser = ClaudeTuiUsageParser::new();
    let snap = parser.parse(FIXTURE).expect("fixture parses");
    assert_eq!(snap.provider, "claude");
    assert_eq!(snap.session_pct, Some(43.0), "session pct");
    assert_eq!(snap.weekly_pct, Some(23.0), "weekly all-models pct");
    assert!(
        snap.session_resets_at_ms.is_some(),
        "session resets_at parsed"
    );
    assert!(
        snap.weekly_resets_at_ms.is_some(),
        "weekly resets_at parsed"
    );
}

#[test]
fn fixture_resets_session_in_future() {
    let parser = ClaudeTuiUsageParser::new();
    let snap = parser.parse(FIXTURE).expect("fixture parses");
    let now = chrono::Utc::now().timestamp_millis();
    let ts = snap.session_resets_at_ms.expect("ts");
    assert!(ts > now, "session resets_at > now");
    assert!(ts - now < 25 * 3600 * 1000, "session resets within 25h");
}

#[test]
fn empty_input_errors() {
    let parser = ClaudeTuiUsageParser::new();
    match parser.parse("") {
        Err(UsageParseError::NothingFound) => {}
        other => panic!("expected NothingFound, got {:?}", other),
    }
    match parser.parse("   \n\n\t  ") {
        Err(UsageParseError::NothingFound) => {}
        other => panic!("expected NothingFound for whitespace, got {:?}", other),
    }
}

#[test]
fn garbled_returns_partial_or_err() {
    let parser = ClaudeTuiUsageParser::new();
    let garbled = "\x1b[31mxxxx\x1b[0m random ANSI garbage with no usage data 🙃 ✨";
    match parser.parse(garbled) {
        Ok(s) => {
            assert!(!s.has_any_field(), "no fields extracted from garbage");
        }
        Err(UsageParseError::NothingFound) => {}
        Err(e) => panic!("unexpected error: {:?}", e),
    }

    let partial = "Current session something 42%usedResets 5:30pm (Europe/Riga)";
    let snap = parser.parse(partial).expect("partial parses");
    assert_eq!(snap.session_pct, Some(42.0));
    assert!(snap.session_resets_at_ms.is_some());
    assert!(snap.weekly_pct.is_none());
}

#[test]
fn raw_text_preserved() {
    let parser = ClaudeTuiUsageParser::new();
    let snap = parser.parse(FIXTURE).expect("fixture parses");
    assert_eq!(snap.raw_text, FIXTURE);
}

#[test]
fn parses_weekly_reset_without_month_day() {
    let parser = ClaudeTuiUsageParser::new();
    let raw = "Current session 100% usedResets 3:30pm (Europe/Kiev)\n\
               Current week (all models) 64% usedResets 4 PM (Europe/Kiev)";
    let snap = parser.parse(raw).expect("usage parses");
    assert_eq!(snap.weekly_pct, Some(64.0));
    assert!(snap.weekly_resets_at_ms.is_some());
}

#[test]
fn parses_weekly_reset_from_current_claude_format() {
    let parser = ClaudeTuiUsageParser::new();
    let raw = "Current session 100% usedResets 3:39pm (Europe/Kiev)\n\
               Current week (all models) 64% usedResets Jun 14, 7:59pm (Europe/Kiev)";
    let snap = parser.parse(raw).expect("usage parses");
    assert_eq!(snap.weekly_pct, Some(64.0));
    assert!(snap.weekly_resets_at_ms.is_some());
}

#[test]
fn rejects_incomplete_session_reset_until_the_frame_finishes() {
    let parser = ClaudeTuiUsageParser::new();
    let incomplete = "Current session 100% usedResets 8:10p (Europe/Kiev)\n\
                      Current week (all models) 55% usedResets Jun 14, 8pm (Europe/Kiev)";
    assert!(matches!(
        parser.parse(incomplete),
        Err(UsageParseError::NothingFound)
    ));

    let complete = "Current session 100% usedResets 8:10pm (Europe/Kiev)\n\
                    Current week (all models) 55% usedResets Jun 14, 8pm (Europe/Kiev)";
    let snap = parser.parse(complete).expect("completed frame parses");
    assert!(snap.session_resets_at_ms.is_some());
    assert!(snap.weekly_resets_at_ms.is_some());
}
