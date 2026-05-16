use crate::client::SocketClient;
use crate::cmd::print::handle_result;
use crate::{build_http, read_api_config, resolve_pane, PaneAction};

pub fn handle_pane(client: &mut SocketClient, json: bool, action: PaneAction) {
    match action {
        PaneAction::List => {
            let result = client.call("pane.list", None);
            match result {
                Ok(value) if !json => {
                    if let Some(arr) = value.as_array() {
                        if arr.is_empty() {
                            println!("(no panes)");
                        } else {
                            for p in arr { println!("{}", format_pane_row(p)); }
                        }
                    } else {
                        handle_result(false, Ok(value));
                    }
                }
                other => handle_result(json, other),
            }
        }
        PaneAction::New { workspace, cwd, title, cmd } => {
            let (base, token) = read_api_config();
            let http = match build_http(&token) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: failed to build HTTP client: {}", e);
                    std::process::exit(1);
                }
            };
            let mut body = serde_json::json!({});
            if let Some(w) = workspace { body["workspace_id"] = serde_json::Value::String(w); }
            if let Some(c) = cwd { body["cwd"] = serde_json::Value::String(c); }
            if let Some(t) = title { body["title"] = serde_json::Value::String(t); }
            let create_result = http.post(format!("{}/api/panes", base))
                .json(&body)
                .send()
                .map_err(|e| e.to_string())
                .and_then(|r| {
                    let status = r.status();
                    let txt = r.text().unwrap_or_default();
                    if status.is_success() {
                        serde_json::from_str::<serde_json::Value>(&txt).map_err(|e| e.to_string())
                    } else {
                        Err(format!("HTTP {}: {}", status.as_u16(), txt))
                    }
                });

            let final_result = match (create_result, cmd) {
                (Ok(resp), Some(cmd_text)) => {
                    let pane_id = resp.get("pane_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    if pane_id.is_empty() {
                        Ok(resp)
                    } else {
                        let exec_url = format!("{}/api/panes/{}/exec", base, pane_id);
                        let exec_body = serde_json::json!({"command": cmd_text});
                        match http.post(&exec_url).json(&exec_body).send() {
                            Ok(r) if r.status().is_success() => Ok(resp),
                            Ok(r) => Err(format!("pane created (id={}) but --cmd failed: HTTP {}: {}",
                                pane_id, r.status().as_u16(), r.text().unwrap_or_default())),
                            Err(e) => Err(format!("pane created (id={}) but --cmd failed: {}", pane_id, e)),
                        }
                    }
                }
                (other, _) => other,
            };
            handle_result(json, final_result);
        }
        PaneAction::Current { last } => {
            let result = client.call("pane.list", None);
            match result {
                Ok(value) => {
                    if let Some(arr) = value.as_array() {
                        let pane = if last {
                            arr.last()
                        } else {
                            arr.iter()
                                .find(|p| p.get("focused").and_then(|v| v.as_bool()).unwrap_or(false))
                                .or_else(|| arr.first())
                        };
                        if let Some(id) = pane.and_then(|p| p.get("id").and_then(|v| v.as_str())) {
                            println!("{}", id);
                        }
                    }
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        PaneAction::Focus { id } => {
            let result = client.call("pane.focus", Some(serde_json::json!({ "pane_id": id })));
            handle_result(json, result);
        }
        PaneAction::Close { id } => {
            let result = client.call("pane.close", Some(serde_json::json!({ "pane_id": id })));
            handle_result(json, result);
        }
        PaneAction::Rename { pane, title } => {
            let pane_id = resolve_pane(client, pane);
            let result = client.call(
                "pane.rename",
                Some(serde_json::json!({ "pane_id": pane_id, "title": title })),
            );
            handle_result(json, result);
        }
        PaneAction::GetText { pane, formatted } => {
            let pane_id = resolve_pane(client, pane);
            let (api_url, api_token) = read_api_config();
            let http = match build_http(&api_token) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: failed to build HTTP client: {}", e);
                    std::process::exit(1);
                }
            };
            let url = format!("{}/api/panes/{}/text?formatted={}", api_url, pane_id, formatted);
            match http.get(url).send() {
                Ok(resp) => {
                    if let Ok(val) = resp.json::<serde_json::Value>() {
                        if let Some(text) = val.get("text").and_then(|v| v.as_str()) {
                            if formatted {
                                print!("{}", text);
                            } else {
                                for line in text.lines() {
                                    if !line.trim().is_empty() {
                                        println!("{}", line);
                                    }
                                }
                            }
                            return;
                        }
                    }
                    eprintln!("No pane text");
                    std::process::exit(1);
                }
                Err(e) => { eprintln!("Get-text request failed: {}", e); std::process::exit(1); }
            }
        }
        PaneAction::Exec { pane, command, timeout } => {
            let pane_id = resolve_pane(client, pane);
            let (api_url, api_token) = read_api_config();
            let http = match build_http(&api_token) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: failed to build HTTP client: {}", e);
                    std::process::exit(1);
                }
            };
            match http.post(format!("{}/api/panes/{}/exec", api_url, pane_id))
                .json(&serde_json::json!({
                    "command": command,
                    "timeout_ms": timeout,
                    "strip_ansi": true,
                }))
                .send()
            {
                Ok(resp) => {
                    if let Ok(val) = resp.json::<serde_json::Value>() {
                        if let Some(output) = val.get("output").and_then(|v| v.as_str()) {
                            print!("{}", output);
                            return;
                        }
                    }
                    eprintln!("Exec failed");
                    std::process::exit(1);
                }
                Err(e) => { eprintln!("Exec request failed: {}", e); std::process::exit(1); }
            }
        }
        PaneAction::Search { pattern } => {
            let result = client.call("pane.list", None);
            match result {
                Ok(value) => {
                    let arr = value.as_array().cloned().unwrap_or_default();
                    print_pane_find_results(json, &pattern, &arr);
                }
                Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
            }
        }
        PaneAction::Status { pane } => {
            let pane_id = resolve_pane(client, pane);
            let (api_url, api_token) = read_api_config();
            let http = match build_http(&api_token) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: failed to build HTTP client: {}", e);
                    std::process::exit(1);
                }
            };
            match http.get(format!("{}/api/panes", api_url)).send() {
                Ok(resp) => {
                    if let Ok(panes) = resp.json::<Vec<serde_json::Value>>() {
                        if let Some(p) = panes.iter().find(|p| p.get("id").and_then(|v| v.as_str()) == Some(&pane_id)) {
                            let at = p.get("agent_type").and_then(|v| v.as_str()).unwrap_or("-");
                            let sid = p.get("agent_session_id").and_then(|v| v.as_str()).unwrap_or("-");
                            let pp = if p.get("pending_prompt").is_some() { "yes" } else { "no" };
                            let cv = p.get("chat_view").and_then(|v| v.as_bool()).unwrap_or(false);
                            println!("pane:    {}", pane_id);
                            println!("agent:   {}", at);
                            println!("session: {}", sid);
                            println!("prompt:  {}", pp);
                            println!("chat:    {}", cv);
                            return;
                        }
                    }
                    eprintln!("Pane not found");
                    std::process::exit(1);
                }
                Err(e) => { eprintln!("Status request failed: {}", e); std::process::exit(1); }
            }
        }
    }
}

/// Rank and print pane-find results. Shared between the local socket path
/// (handle_pane) and the mesh-relayed path (main.rs intercepts `--mesh
/// pane find` so the ranker runs over the peer's list).
pub fn print_pane_find_results(json: bool, pattern: &str, panes: &[serde_json::Value]) {
    let scored = rank_panes(pattern, panes);
    if json {
        let out: Vec<&serde_json::Value> = scored.iter().map(|(_, p)| *p).collect();
        println!("{}", serde_json::to_string_pretty(&out).unwrap());
    } else if scored.is_empty() {
        println!("No panes match: {}", pattern);
    } else {
        for (_, p) in &scored {
            println!("{}", format_pane_row(p));
        }
    }
}

/// Pure scoring function — separated so tests can exercise without IO.
/// Searches id-prefix, agent_session_name (friendly), tab.title (workspace-
/// tree label), pane.title, agent_type, and agent_session_model. tab.title
/// is the canonical name the user sees in the sidebar. Cross-script search
/// via `deunicode`: typing "далтон" / "далтон" matches "Dalton", and a Latin
/// needle still hits a Cyrillic-titled tab — only one of the two has to be
/// ASCII for a match to land.
pub fn rank_panes<'a>(pattern: &str, panes: &'a [serde_json::Value]) -> Vec<(i32, &'a serde_json::Value)> {
    let needle = pattern.to_lowercase();
    let needle_translit = translit(&needle);
    let needle_compact = strip_separators(&needle);
    let needle_translit_compact = strip_separators(&needle_translit);
    let mut scored: Vec<(i32, &serde_json::Value)> = panes.iter()
        .filter_map(|p| {
            let id = field(p, "id");
            let title = field(p, "title");
            let tab_title = field(p, "tab_title");
            let ws_name = field(p, "workspace_name");
            let sname = field(p, "agent_session_name");
            let agent = field(p, "agent_type");
            let model = field(p, "agent_session_model");
            let mut score = 0i32;
            // id prefix only counts at ≥4 chars — one-letter needles like "a"
            // otherwise match ~6% of all UUIDs and drown the real result.
            if needle.len() >= 4 && id.starts_with(&needle) { score += 100; }
            score += score_field(&sname, &needle, &needle_translit, 80, 50);
            score += score_field(&tab_title, &needle, &needle_translit, 75, 45);
            if !needle_compact.is_empty() || !needle_translit_compact.is_empty() {
                let sname_c = strip_separators(&sname);
                let sname_c_translit = strip_separators(&translit(&sname));
                if !sname_c.is_empty() && (
                    (!needle_compact.is_empty() && sname_c.contains(&needle_compact))
                    || (!needle_translit_compact.is_empty() && sname_c_translit.contains(&needle_translit_compact))
                ) { score += 40; }
                let tab_c = strip_separators(&tab_title);
                let tab_c_translit = strip_separators(&translit(&tab_title));
                if !tab_c.is_empty() && (
                    (!needle_compact.is_empty() && tab_c.contains(&needle_compact))
                    || (!needle_translit_compact.is_empty() && tab_c_translit.contains(&needle_translit_compact))
                ) { score += 38; }
            }
            score += score_field(&title, &needle, &needle_translit, 35, 20);
            if !ws_name.is_empty() {
                let ws_translit = translit(&ws_name);
                if ws_name.contains(&needle) || ws_translit.contains(&needle_translit) {
                    score += 15;
                }
            }
            if !agent.is_empty() && agent.contains(&needle) { score += 10; }
            if !model.is_empty() && model.contains(&needle) { score += 10; }
            if score > 0 { Some((score, p)) } else { None }
        })
        .collect();
    // Stable tie-break by id so identical scores keep a deterministic order.
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0).then_with(|| {
            let ai = a.1.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let bi = b.1.get("id").and_then(|v| v.as_str()).unwrap_or("");
            ai.cmp(bi)
        })
    });
    scored
}

fn field(v: &serde_json::Value, key: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("").to_lowercase()
}

fn strip_separators(s: &str) -> String {
    // Apostrophes get inserted by `deunicode` for Cyrillic soft signs
    // (Дальтон → Dal'ton); strip them so the compact match still hits a
    // plain "Dalton" tab. Same for whitespace / dash / underscore which
    // commonly differ between user input and stored titles.
    s.chars()
        .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_' && *c != '\'')
        .collect()
}

/// ASCII-lowercase transliteration. `deunicode` already handles Cyrillic →
/// Latin (Далтон → Dalton), Greek, Japanese, etc. Returns the lowered form
/// so the result is symmetric with `needle.to_lowercase()`.
fn translit(s: &str) -> String {
    deunicode::deunicode(s).to_lowercase()
}

/// Score one haystack field with both exact-match (high) and contains
/// (medium) tiers, falling back to the transliterated form if the raw
/// match misses. Returning 0 means no contribution.
fn score_field(haystack: &str, needle: &str, needle_translit: &str, exact: i32, contains: i32) -> i32 {
    if haystack.is_empty() { return 0; }
    if haystack == needle { return exact; }
    let h_translit = translit(haystack);
    if h_translit == needle_translit && !needle_translit.is_empty() { return exact; }
    if haystack.contains(needle) { return contains; }
    if !needle_translit.is_empty() && h_translit.contains(needle_translit) { return contains; }
    0
}

/// One-line human row used by both `pane list` (when we switch off the
/// default kv printer) and `pane find`. Shows id  tab/title  [provider/model]
/// so the user gets the same info they see in the sidebar.
pub fn format_pane_row(p: &serde_json::Value) -> String {
    let id = p.get("id").and_then(|v| v.as_str()).unwrap_or("?");
    let tab_title = p.get("tab_title").and_then(|v| v.as_str()).unwrap_or("");
    let pane_title = p.get("title").and_then(|v| v.as_str()).unwrap_or("");
    let label = if !tab_title.is_empty() && tab_title != pane_title {
        format!("{}  ({})", tab_title, pane_title)
    } else if !tab_title.is_empty() {
        tab_title.to_string()
    } else {
        pane_title.to_string()
    };
    let agent = p.get("agent_type").and_then(|v| v.as_str()).unwrap_or("-");
    let model = p.get("agent_session_model").and_then(|v| v.as_str()).unwrap_or("");
    let model_part = if model.is_empty() { String::new() } else { format!("/{}", model) };
    let sname = p.get("agent_session_name").and_then(|v| v.as_str()).unwrap_or("");
    let sname_part = if sname.is_empty() { String::new() } else { format!("  session={}", sname) };
    let ws = p.get("workspace_name").and_then(|v| v.as_str()).unwrap_or("");
    let ws_part = if ws.is_empty() { String::new() } else { format!("  ws={}", ws) };
    format!("{}  {} [{}{}]{}{}", id, label, agent, model_part, sname_part, ws_part)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_pane(id: &str, title: &str, agent: Option<&str>, sname: Option<&str>) -> serde_json::Value {
        let mut p = json!({"id": id, "title": title});
        if let Some(a) = agent { p["agent_type"] = json!(a); }
        if let Some(n) = sname { p["agent_session_name"] = json!(n); }
        p
    }

    fn rank_ids(needle: &str, panes: &[serde_json::Value]) -> Vec<String> {
        rank_panes(needle, panes)
            .into_iter()
            .map(|(_, p)| p.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string())
            .collect()
    }

    #[test]
    fn short_needle_does_not_match_id_prefix() {
        let panes = vec![
            make_pane("aaaaa1", "OtherPane", Some("claude"), None),
            make_pane("bbbb22", "a", None, None),
        ];
        let ranked = rank_ids("a", &panes);
        assert_eq!(ranked.first().map(String::as_str), Some("bbbb22"),
            "exact title match must beat id-prefix when needle is short");
    }

    #[test]
    fn long_needle_id_prefix_still_wins() {
        let panes = vec![
            make_pane("abcd1234", "ignored", None, None),
            make_pane("ffffffff", "abcd", None, None),
        ];
        let ranked = rank_ids("abcd", &panes);
        assert_eq!(ranked.first().map(String::as_str), Some("abcd1234"),
            "id-prefix dominates at needle length ≥ 4");
    }

    #[test]
    fn tie_break_is_stable_by_id() {
        let panes = vec![
            make_pane("zzzz9999", "foo-bar", None, None),
            make_pane("aaaa1111", "foo-baz", None, None),
        ];
        let ranked = rank_ids("foo", &panes);
        assert_eq!(ranked, vec!["aaaa1111".to_string(), "zzzz9999".to_string()]);
    }

    #[test]
    fn tab_title_matches_friendly_name() {
        // Regression: user searches "dalton" (workspace-tree label), expects
        // the matching pane back. Before tab_title was wired in, the matcher
        // ignored it and returned "No panes match".
        let panes = vec![
            json!({"id": "11111111", "title": "claude-with-text", "tab_title": "Dalton"}),
            json!({"id": "22222222", "title": "silent-claude",    "tab_title": "Thomson"}),
        ];
        let ranked = rank_ids("dalton", &panes);
        assert_eq!(ranked.first().map(String::as_str), Some("11111111"),
            "tab_title must be searched — that's the name the user sees");
    }

    #[test]
    fn tab_title_contains_outranks_title_contains() {
        // Tab title is more user-facing than the raw pane.title; "fib" should
        // hit Fibonacci's tab over a pane that just has "fib" in its title.
        let panes = vec![
            json!({"id": "aa11", "title": "ignore-me-fib"}),
            json!({"id": "bb22", "title": "claude", "tab_title": "Fibonacci"}),
        ];
        let ranked = rank_ids("fib", &panes);
        assert_eq!(ranked.first().map(String::as_str), Some("bb22"));
    }

    #[test]
    fn model_field_is_searchable() {
        // Asking for "sonnet" or "haiku" finds panes currently running that
        // model — covers the "which tabs have opus available?" workflow.
        let panes = vec![
            json!({"id": "p1111111", "title": "t1", "agent_type": "claude",  "agent_session_model": "Sonnet"}),
            json!({"id": "p2222222", "title": "t2", "agent_type": "opencode","agent_session_model": "Deepseek"}),
        ];
        let ranked = rank_ids("sonnet", &panes);
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
        let row = format_pane_row(&p);
        assert!(row.contains("Dalton"), "row missing tab_title: {}", row);
        assert!(row.contains("(claude-with-text)"), "row missing pane.title: {}", row);
        assert!(row.contains("claude"), "row missing agent: {}", row);
        assert!(row.contains("Sonnet"), "row missing model: {}", row);
        assert!(row.contains("ws=WS-claude-renamed"), "row missing workspace: {}", row);
    }

    #[test]
    fn cyrillic_needle_matches_latin_tab_title() {
        // User types tab name in Russian — should still find the Dalton tab.
        let panes = vec![
            json!({"id": "11111111", "title": "claude", "tab_title": "Dalton"}),
            json!({"id": "22222222", "title": "claude", "tab_title": "Thomson"}),
        ];
        let ranked = rank_ids("Далтон", &panes);
        assert_eq!(ranked.first().map(String::as_str), Some("11111111"),
            "cyrillic 'Далтон' must transliterate to 'Dalton' and match");
    }

    #[test]
    fn latin_needle_matches_cyrillic_tab_title() {
        // Reverse direction: pane name is Cyrillic, user types Latin needle.
        // deunicode renders Андрей → "Andrei" (Library of Congress style), so
        // the test uses that romanization.
        let panes = vec![
            json!({"id": "33333333", "title": "claude", "tab_title": "Андрей"}),
            json!({"id": "44444444", "title": "claude", "tab_title": "Misha"}),
        ];
        let ranked = rank_ids("andrei", &panes);
        assert_eq!(ranked.first().map(String::as_str), Some("33333333"),
            "latin 'andrei' must match the transliterated form of 'Андрей'");
    }

    #[test]
    fn human_row_collapses_when_tab_title_equals_pane_title() {
        let p = json!({
            "id": "abcd1234",
            "title": "Main",
            "tab_title": "Main",
            "agent_type": "claude",
        });
        let row = format_pane_row(&p);
        // No "(Main)" duplicate — single label only.
        assert!(!row.contains("(Main)"), "duplicate label in row: {}", row);
    }
}
