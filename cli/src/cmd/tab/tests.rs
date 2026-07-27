use super::*;
use serde_json::json;

fn make_tab(id: &str, title: &str, agent: Option<&str>, sname: Option<&str>) -> serde_json::Value {
    let mut p = json!({"id": id, "title": title});
    if let Some(a) = agent {
        p["agent_type"] = json!(a);
    }
    if let Some(n) = sname {
        p["agent_session_name"] = json!(n);
    }
    p
}

fn rank_ids(needle: &str, tabs: &[serde_json::Value]) -> Vec<String> {
    rank_tabs(needle, tabs)
        .into_iter()
        .map(|(_, p)| {
            p.get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string()
        })
        .collect()
}

#[test]
fn render_tree_groups_by_workspace_and_nests_children() {
    let tabs = vec![
        json!({"id":"aaaa","tab_title":"MainX","agent_session_model":"Sonnet","workspace_name":"General","workspace_id":"ws1"}),
        json!({"id":"bbbb","tab_title":"Torvalds","agent_session_model":"Opus","workspace_name":"General","workspace_id":"ws1"}),
        json!({"id":"cccc","tab_title":"scout","agent_session_model":"Sonnet","workspace_name":"General","workspace_id":"ws1","parent_tab_id":"bbbb"}),
        json!({"id":"dddd","tab_title":"devbox","agent_session_model":"Sonnet","workspace_name":"codeterm","workspace_id":"ws2"}),
    ];
    let out = render_tab_tree(&tabs);
    assert!(
        out.contains("General  ws1"),
        "workspace header shows name + ws id:\n{}",
        out
    );
    assert!(
        out.contains("codeterm  ws2"),
        "second workspace header:\n{}",
        out
    );

    let lines: Vec<&str> = out.lines().collect();
    let torvalds = lines.iter().position(|l| l.contains("Torvalds")).unwrap();
    let scout = lines.iter().position(|l| l.contains("scout")).unwrap();
    assert_eq!(
        scout,
        torvalds + 1,
        "child renders directly under its parent:\n{}",
        out
    );
    let indent = |l: &str| l.len() - l.trim_start().len();
    assert!(
        indent(lines[scout]) > indent(lines[torvalds]),
        "child indented deeper:\n{}",
        out
    );
    assert!(
        lines[torvalds].trim_start().starts_with("bbbb"),
        "id comes first: {}",
        lines[torvalds]
    );
}

#[test]
fn render_tree_falls_back_to_agent_type_without_model() {
    let tabs = vec![
        json!({"id":"eeee","tab_title":"plain","workspace_name":"W","workspace_id":"w"}),
        json!({"id":"ffff","tab_title":"coder","agent_type":"claude","workspace_name":"W","workspace_id":"w"}),
    ];
    let out = render_tab_tree(&tabs);
    // No model + no agent_type → bare id + name (no trailing junk).
    assert!(
        out.contains("eeee plain\n"),
        "plain Tab is id + name only:\n{}",
        out
    );
    // agent_type fills in when the session model is absent.
    assert!(
        out.contains("claude"),
        "agent_type shown when model missing:\n{}",
        out
    );
}

#[test]
fn render_tree_includes_description() {
    let tabs = vec![json!({
        "id":"eeee",
        "tab_title":"plain",
        "description":"running tests",
        "workspace_name":"W",
        "workspace_id":"w"
    })];
    let out = render_tab_tree(&tabs);
    assert!(
        out.contains("eeee plain - running tests\n"),
        "description missing from Tab list:\n{}",
        out
    );
}

#[test]
fn short_needle_does_not_match_id_prefix() {
    let tabs = vec![
        make_tab("aaaaa1", "OtherTab", Some("claude"), None),
        make_tab("bbbb22", "a", None, None),
    ];
    let ranked = rank_ids("a", &tabs);
    assert_eq!(
        ranked.first().map(String::as_str),
        Some("bbbb22"),
        "exact title match must beat id-prefix when needle is short"
    );
}

#[test]
fn long_needle_id_prefix_still_wins() {
    let tabs = vec![
        make_tab("abcd1234", "ignored", None, None),
        make_tab("ffffffff", "abcd", None, None),
    ];
    let ranked = rank_ids("abcd", &tabs);
    assert_eq!(
        ranked.first().map(String::as_str),
        Some("abcd1234"),
        "id-prefix dominates at needle length ≥ 4"
    );
}

#[test]
fn tie_break_is_stable_by_id() {
    let tabs = vec![
        make_tab("zzzz9999", "foo-bar", None, None),
        make_tab("aaaa1111", "foo-baz", None, None),
    ];
    let ranked = rank_ids("foo", &tabs);
    assert_eq!(ranked, vec!["aaaa1111".to_string(), "zzzz9999".to_string()]);
}

#[test]
fn tab_title_matches_friendly_name() {
    // Regression: use workspace-tree tab title in matching so labels like "dalton" still find the expected tab.
    let tabs = vec![
        json!({"id": "11111111", "title": "claude-with-text", "tab_title": "Dalton"}),
        json!({"id": "22222222", "title": "silent-claude",    "tab_title": "Thomson"}),
    ];
    let ranked = rank_ids("dalton", &tabs);
    assert_eq!(
        ranked.first().map(String::as_str),
        Some("11111111"),
        "tab_title must be searched — that's the name the user sees"
    );
}

#[test]
fn tab_title_contains_outranks_title_contains() {
    // Tab title is more user-facing than the raw tab.title; "fib" should hit Fibonacci's tab over a tab that just has "fib" in its title.
    let tabs = vec![
        json!({"id": "aa11", "title": "ignore-me-fib"}),
        json!({"id": "bb22", "title": "claude", "tab_title": "Fibonacci"}),
    ];
    let ranked = rank_ids("fib", &tabs);
    assert_eq!(ranked.first().map(String::as_str), Some("bb22"));
}

#[test]
fn model_field_is_searchable() {
    // Asking for "sonnet" or "haiku" finds tabs currently running that model — covers the "which tabs have opus available?" workflow.
    let tabs = vec![
        json!({"id": "p1111111", "title": "t1", "agent_type": "claude",  "agent_session_model": "Sonnet"}),
        json!({"id": "p2222222", "title": "t2", "agent_type": "opencode","agent_session_model": "Deepseek"}),
    ];
    let ranked = rank_ids("sonnet", &tabs);
    assert_eq!(ranked.first().map(String::as_str), Some("p1111111"));
}

#[test]
fn human_row_shows_tab_title_with_model() {
    let p = json!({
        "id": "abcd1234",
        "title": "claude-with-text",
        "tab_title": "Dalton",
        "agent_type": "claude",
        "agent_session_model": "Sonnet",
        "workspace_name": "WS-claude-renamed",
    });
    let row = format_tab_row(&p);
    assert!(row.contains("Dalton"), "row missing tab_title: {}", row);
    // One label only: tab_title wins, so the raw tab.title must NOT leak in.
    assert!(
        !row.contains("claude-with-text"),
        "tab_title should win over tab.title: {}",
        row
    );
    assert!(row.contains("claude"), "row missing agent: {}", row);
    assert!(row.contains("Sonnet"), "row missing model: {}", row);
    assert!(
        row.contains("ws=WS-claude-renamed"),
        "row missing workspace: {}",
        row
    );
}

#[test]
fn cyrillic_needle_matches_latin_tab_title() {
    // User types tab name in Russian — should still find the Dalton tab.
    let tabs = vec![
        json!({"id": "11111111", "title": "claude", "tab_title": "Dalton"}),
        json!({"id": "22222222", "title": "claude", "tab_title": "Thomson"}),
    ];
    let ranked = rank_ids("Далтон", &tabs);
    assert_eq!(
        ranked.first().map(String::as_str),
        Some("11111111"),
        "cyrillic 'Далтон' must transliterate to 'Dalton' and match"
    );
}

#[test]
fn latin_needle_matches_cyrillic_tab_title() {
    // Reverse matching uses deunicode romanization so Cyrillic tab names can be found by Latin input.
    let tabs = vec![
        json!({"id": "33333333", "title": "claude", "tab_title": "Андрей"}),
        json!({"id": "44444444", "title": "claude", "tab_title": "Misha"}),
    ];
    let ranked = rank_ids("andrei", &tabs);
    assert_eq!(
        ranked.first().map(String::as_str),
        Some("33333333"),
        "latin 'andrei' must match the transliterated form of 'Андрей'"
    );
}

#[test]
fn human_row_collapses_when_tab_title_equals_tab_title() {
    let p = json!({
        "id": "abcd1234",
        "title": "Main",
        "tab_title": "Main",
        "agent_type": "claude",
    });
    let row = format_tab_row(&p);
    // No "(Main)" duplicate — single label only.
    assert!(!row.contains("(Main)"), "duplicate label in row: {}", row);
}
