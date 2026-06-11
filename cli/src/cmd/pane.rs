use crate::client::SocketClient;
use crate::cmd::print::handle_result;
use crate::{build_http, read_api_config, resolve_pane, PaneAction};

pub fn handle_pane(client: &mut SocketClient, json: bool, action: PaneAction) {
    match action {
        PaneAction::List { agents } => {
            match client.call("pane.list", None) {
                Ok(value) => {
                    let Some(arr) = value.as_array() else {
                        handle_result(json, Ok(value));
                        return;
                    };
                    let mut list = arr.clone();
                    if agents {
                        list.retain(|p| {
                            p.get("agent_type")
                                .and_then(|v| v.as_str())
                                .map_or(false, |s| !s.is_empty())
                        });
                    }
                    if json {
                        println!("{}", serde_json::to_string_pretty(&list).unwrap());
                    } else if list.is_empty() {
                        println!("(no panes)");
                    } else {
                        print!("{}", render_pane_tree(&list));
                    }
                }
                other => handle_result(json, other),
            }
        }
        PaneAction::New { workspace, cwd, title, cmd, eager } => {
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
            if eager { body["eager"] = serde_json::Value::Bool(true); }
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
        PaneAction::Stop { pane } => {
            let pane_id = resolve_pane(client, pane);
            let (api_url, api_token) = read_api_config();
            let http = match build_http(&api_token) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: failed to build HTTP client: {}", e);
                    std::process::exit(1);
                }
            };
            match http
                .post(format!("{}/api/panes/{}/stop", api_url, pane_id))
                .header("Content-Type", "application/json")
                .body("{}")
                .send()
            {
                Ok(resp) => {
                    let status = resp.status();
                    let body: serde_json::Value = resp.json().unwrap_or_else(|_| serde_json::json!({}));
                    if json {
                        println!("{}", serde_json::to_string_pretty(&body).unwrap());
                    } else if !status.is_success() {
                        eprintln!("Stop failed: HTTP {}", status);
                        std::process::exit(1);
                    } else if body.get("stopped").and_then(|v| v.as_bool()).unwrap_or(false) {
                        println!("Stopped pane {}", pane_id);
                    } else {
                        println!("Pane {} has no stop keystroke declared", pane_id);
                    }
                }
                Err(e) => { eprintln!("Stop request failed: {}", e); std::process::exit(1); }
            }
        }
        PaneAction::Chat { pane, since } => {
            let pane_id = resolve_pane(client, pane);
            let (api_url, api_token) = read_api_config();
            let http = match build_http(&api_token) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Error: failed to build HTTP client: {}", e);
                    std::process::exit(1);
                }
            };
            let url = format!("{}/api/panes/{}/chat?since={}", api_url, pane_id, since);
            match http.get(url).send().and_then(|r| r.json::<serde_json::Value>()) {
                Ok(v) => handle_result(json, Ok(v)),
                Err(e) => { eprintln!("Error: chat read failed: {}", e); std::process::exit(1); }
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

/// Fuzzy ranker over id / tab.title / session name / pane.title / agent / model. Cross-script via deunicode.
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
            // id prefix only counts at ≥4 chars — one-letter needles like "a" otherwise match ~6% of all UUIDs and drown the real result.
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

/// Like `field` but preserves case — for display, not fuzzy matching.
fn sfield<'a>(v: &'a serde_json::Value, key: &str) -> &'a str {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("")
}

fn strip_separators(s: &str) -> String {
    // Apostrophes get inserted by `deunicode` for Cyrillic soft signs (Дальтон → Dal'ton); strip them so the compact match still hits a plain
    // "Dalton" tab. Same for whitespace / dash / underscore which commonly differ between user input and stored titles.
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
    // One label — the sidebar name. Tab title wins (that's what the user sees and what `pane rename` updates); falls back to pane.title only when the tab title is missing.
    let label = {
        let tab_title = p.get("tab_title").and_then(|v| v.as_str()).unwrap_or("");
        let pane_title = p.get("title").and_then(|v| v.as_str()).unwrap_or("");
        if !tab_title.is_empty() { tab_title } else { pane_title }
    };
    let agent = p.get("agent_type").and_then(|v| v.as_str()).unwrap_or("-");
    let model = p.get("agent_session_model").and_then(|v| v.as_str()).unwrap_or("");
    let model_part = if model.is_empty() { String::new() } else { format!("/{}", model) };
    let ws = p.get("workspace_name").and_then(|v| v.as_str()).unwrap_or("");
    let ws_part = if ws.is_empty() { String::new() } else { format!("  ws={}", ws) };
    format!("{}  {} [{}{}]{}", id, label, agent, model_part, ws_part)
}

/// Human `pane list`: workspaces as headers, panes as `<id> <name> <model>`
/// rows, sub-agents nested under their parent by `parent_pane_id`. Compact
/// on purpose — this output is read by agents too, so every line costs tokens.
pub fn render_pane_tree(panes: &[serde_json::Value]) -> String {
    struct Row {
        id: String,
        label: String,
        model: String,
        parent: Option<String>,
    }
    // Group by workspace. BTreeMap keeps a stable (name, id) ordering so the output doesn't reshuffle between calls (panes arrive
    // HashMap-unordered).
    let mut groups: std::collections::BTreeMap<(String, String), Vec<Row>> =
        std::collections::BTreeMap::new();
    for p in panes {
        let id = sfield(p, "id").to_string();
        if id.is_empty() {
            continue;
        }
        let tab_title = sfield(p, "tab_title");
        let label = if !tab_title.is_empty() { tab_title } else { sfield(p, "title") }.to_string();
        let model_raw = sfield(p, "agent_session_model");
        let model = if !model_raw.is_empty() { model_raw } else { sfield(p, "agent_type") }.to_string();
        let parent = p
            .get("parent_pane_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let ws_name = sfield(p, "workspace_name").to_string();
        let ws_id = sfield(p, "workspace_id").to_string();
        groups
            .entry((ws_name, ws_id))
            .or_default()
            .push(Row { id, label, model, parent });
    }

    let mut out = String::new();
    for (gi, ((ws_name, ws_id), rows)) in groups.iter().enumerate() {
        if gi > 0 {
            out.push('\n');
        }
        let name = if ws_name.is_empty() { "(detached)" } else { ws_name.as_str() };
        if ws_id.is_empty() {
            out.push_str(&format!("{}\n", name));
        } else {
            out.push_str(&format!("{}  {}\n", name, ws_id));
        }

        // Nest by parent within the workspace. A pane whose parent lives in another workspace (or has none) is a root here.
        let ids: std::collections::HashSet<&str> = rows.iter().map(|r| r.id.as_str()).collect();
        let mut children: std::collections::HashMap<&str, Vec<usize>> =
            std::collections::HashMap::new();
        let mut roots: Vec<usize> = Vec::new();
        for (i, r) in rows.iter().enumerate() {
            match &r.parent {
                Some(par) if ids.contains(par.as_str()) => {
                    children.entry(par.as_str()).or_default().push(i);
                }
                _ => roots.push(i),
            }
        }
        let label_w = rows.iter().map(|r| r.label.chars().count()).max().unwrap_or(0).min(24);
        let by_label = |a: &usize, b: &usize| rows[*a].label.cmp(&rows[*b].label);
        roots.sort_by(by_label);

        // Iterative DFS so deep nesting can't blow the stack.
        let mut stack: Vec<(usize, usize)> = roots.iter().rev().map(|&i| (i, 0usize)).collect();
        while let Some((i, depth)) = stack.pop() {
            let r = &rows[i];
            let indent = "  ".repeat(depth + 1);
            if r.model.is_empty() {
                out.push_str(&format!("{}{} {}\n", indent, r.id, r.label));
            } else {
                out.push_str(&format!("{}{} {:<w$} {}\n", indent, r.id, r.label, r.model, w = label_w));
            }
            if let Some(kids) = children.get(r.id.as_str()) {
                let mut kids = kids.clone();
                kids.sort_by(by_label);
                for &k in kids.iter().rev() {
                    stack.push((k, depth + 1));
                }
            }
        }
    }
    out
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
    fn render_tree_groups_by_workspace_and_nests_children() {
        let panes = vec![
            json!({"id":"aaaa","tab_title":"MainX","agent_session_model":"Sonnet","workspace_name":"General","workspace_id":"ws1"}),
            json!({"id":"bbbb","tab_title":"Torvalds","agent_session_model":"Opus","workspace_name":"General","workspace_id":"ws1"}),
            json!({"id":"cccc","tab_title":"scout","agent_session_model":"Sonnet","workspace_name":"General","workspace_id":"ws1","parent_pane_id":"bbbb"}),
            json!({"id":"dddd","tab_title":"devbox","agent_session_model":"Sonnet","workspace_name":"codeterm","workspace_id":"ws2"}),
        ];
        let out = render_pane_tree(&panes);
        assert!(out.contains("General  ws1"), "workspace header shows name + ws id:\n{}", out);
        assert!(out.contains("codeterm  ws2"), "second workspace header:\n{}", out);

        let lines: Vec<&str> = out.lines().collect();
        let torvalds = lines.iter().position(|l| l.contains("Torvalds")).unwrap();
        let scout = lines.iter().position(|l| l.contains("scout")).unwrap();
        assert_eq!(scout, torvalds + 1, "child renders directly under its parent:\n{}", out);
        let indent = |l: &str| l.len() - l.trim_start().len();
        assert!(indent(lines[scout]) > indent(lines[torvalds]), "child indented deeper:\n{}", out);
        assert!(lines[torvalds].trim_start().starts_with("bbbb"), "id comes first: {}", lines[torvalds]);
    }

    #[test]
    fn render_tree_falls_back_to_agent_type_without_model() {
        let panes = vec![
            json!({"id":"eeee","tab_title":"plain","workspace_name":"W","workspace_id":"w"}),
            json!({"id":"ffff","tab_title":"coder","agent_type":"claude","workspace_name":"W","workspace_id":"w"}),
        ];
        let out = render_pane_tree(&panes);
        // No model + no agent_type → bare id + name (no trailing junk).
        assert!(out.contains("eeee plain\n"), "plain pane is id + name only:\n{}", out);
        // agent_type fills in when the session model is absent.
        assert!(out.contains("claude"), "agent_type shown when model missing:\n{}", out);
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
        // Regression: use workspace-tree tab title in matching so labels like "dalton" still find the expected pane.
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
        // Tab title is more user-facing than the raw pane.title; "fib" should hit Fibonacci's tab over a pane that just has "fib" in its title.
        let panes = vec![
            json!({"id": "aa11", "title": "ignore-me-fib"}),
            json!({"id": "bb22", "title": "claude", "tab_title": "Fibonacci"}),
        ];
        let ranked = rank_ids("fib", &panes);
        assert_eq!(ranked.first().map(String::as_str), Some("bb22"));
    }

    #[test]
    fn model_field_is_searchable() {
        // Asking for "sonnet" or "haiku" finds panes currently running that model — covers the "which tabs have opus available?" workflow.
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
        // One label only: tab_title wins, so the raw pane.title must NOT leak in.
        assert!(!row.contains("claude-with-text"), "tab_title should win over pane.title: {}", row);
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
        // Reverse matching uses deunicode romanization so Cyrillic pane names can be found by Latin input.
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
