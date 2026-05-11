use super::*;
use crate::agent::usage_parser::{BreakdownDimension, UsageParseError, UsageParser};

const FIXTURE: &str = include_str!("../fixtures/usage_v2_1.txt");

#[test]
fn parses_fixture_v2_1() {
    let parser = ClaudeTuiUsageParser::new();
    let snap = parser.parse(FIXTURE).expect("fixture parses");
    assert_eq!(snap.provider, "claude");
    assert_eq!(snap.session_pct, Some(43.0), "session pct");
    assert_eq!(snap.weekly_pct, Some(23.0), "weekly all-models pct");
    assert_eq!(snap.weekly_sonnet_pct, Some(3.0), "sonnet pct");
    assert_eq!(snap.session_cost_usd, Some(0.0));
    assert_eq!(snap.session_api_duration_s, Some(0));
    assert_eq!(snap.session_wall_duration_s, Some(26));
    assert_eq!(snap.session_lines_added, Some(0));
    assert_eq!(snap.session_lines_removed, Some(0));
    assert_eq!(snap.session_tokens_input, Some(0));
    assert_eq!(snap.session_tokens_output, Some(0));
    assert_eq!(snap.session_tokens_cache_read, Some(0));
    assert_eq!(snap.session_tokens_cache_write, Some(0));
    assert!(snap.session_resets_at_ms.is_some(), "session resets_at parsed");
    assert!(snap.weekly_resets_at_ms.is_some(), "weekly resets_at parsed");
    assert!(snap.weekly_sonnet_resets_at_ms.is_some(), "sonnet resets_at parsed");
}

#[test]
fn fixture_breakdown_covers_all_dimensions() {
    let parser = ClaudeTuiUsageParser::new();
    let snap = parser.parse(FIXTURE).expect("fixture parses");
    let has = |d: BreakdownDimension| snap.breakdown.iter().any(|r| r.dimension == d);
    assert!(has(BreakdownDimension::Skill), "skill row present");
    assert!(has(BreakdownDimension::Subagent), "subagent row present");
    assert!(has(BreakdownDimension::Plugin), "plugin row present");
    assert!(has(BreakdownDimension::Heuristic), "heuristic row present");

    let subagent_keys: Vec<&str> = snap
        .breakdown
        .iter()
        .filter(|r| r.dimension == BreakdownDimension::Subagent)
        .map(|r| r.key.as_str())
        .collect();
    assert!(subagent_keys.iter().any(|k| *k == "backend-dev"), "backend-dev subagent");

    let plugin_keys: Vec<&str> = snap
        .breakdown
        .iter()
        .filter(|r| r.dimension == BreakdownDimension::Plugin)
        .map(|r| r.key.as_str())
        .collect();
    assert!(plugin_keys.iter().any(|k| *k == "superpowers"), "superpowers plugin");
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
fn missing_sonnet_block_does_not_error() {
    let mut without_sonnet = String::new();
    for line in FIXTURE.lines() {
        if line.contains("Son") && line.contains("only")
            || line.contains("Son") && line.contains("nly")
        {
            continue;
        }
        without_sonnet.push_str(line);
        without_sonnet.push('\n');
    }
    let stripped = strip_sonnet_section(&without_sonnet);
    let parser = ClaudeTuiUsageParser::new();
    let snap = parser.parse(&stripped).expect("should still parse");
    assert!(snap.weekly_sonnet_pct.is_none(), "sonnet pct gone");
    assert_eq!(snap.session_pct, Some(43.0), "session pct still parsed");
    assert_eq!(snap.weekly_pct, Some(23.0), "weekly pct still parsed");
}

fn strip_sonnet_section(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    if let Some(start) = lower.find("current week (son") {
        if let Some(rel_end) = lower[start..].find("\n\n") {
            let mut out = String::new();
            out.push_str(&s[..start]);
            out.push_str(&s[start + rel_end..]);
            return out;
        }
    }
    s.to_string()
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
