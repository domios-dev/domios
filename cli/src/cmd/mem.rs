use crate::cmd::print::{handle_result, print_history_timeline, print_taxonomy};
use crate::{
    build_http, data_dir_name, read_api_config, MemAction, MemGraphAction, MemSkillAction,
    MemWorkerAction,
};

// Lightweight frontmatter parser for SKILL.md.
// Recognises `title:`, `area:`, `tags:` (comma-separated) between leading `---` lines.
fn strip_skill_frontmatter(body: &str) -> &str {
    let mut lines = body.lines();
    if lines.next().map(|l| l.trim()) != Some("---") { return body; }
    let mut consumed = body.lines().next().unwrap_or("").len() + 1;
    for line in lines {
        consumed += line.len() + 1;
        if line.trim() == "---" {
            return body.get(consumed..).unwrap_or("");
        }
    }
    body
}

fn parse_skill_frontmatter(body: &str) -> (Option<String>, Option<String>, Vec<String>) {
    let mut lines = body.lines();
    if lines.next().map(|l| l.trim()) != Some("---") {
        return (None, None, Vec::new());
    }
    let mut title = None;
    let mut area = None;
    let mut tags = Vec::new();
    for line in lines {
        let t = line.trim();
        if t == "---" { break; }
        if let Some(v) = t.strip_prefix("title:") {
            title = Some(v.trim().trim_matches('"').to_string());
        } else if let Some(v) = t.strip_prefix("area:") {
            area = Some(v.trim().trim_matches('"').to_string());
        } else if let Some(v) = t.strip_prefix("tags:") {
            tags = v.trim().trim_matches('[').trim_matches(']')
                .split(',').map(|s| s.trim().trim_matches('"').to_string())
                .filter(|s| !s.is_empty()).collect();
        }
    }
    (title, area, tags)
}

fn mem_get_request(http: &reqwest::blocking::Client, url: &str) -> Result<serde_json::Value, String> {
    let resp = http.get(url).send().map_err(|e| e.to_string())?;
    let status = resp.status();
    let val: serde_json::Value = resp.json().unwrap_or_default();
    if !status.is_success() {
        return Err(val.get("message").and_then(|v| v.as_str()).map(String::from)
            .unwrap_or_else(|| format!("HTTP {}", status)));
    }
    Ok(val)
}

fn mem_post_request(
    http: &reqwest::blocking::Client,
    url: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, String> {
    let resp = http.post(url).json(body).send().map_err(|e| e.to_string())?;
    let status = resp.status();
    let text = resp.text().unwrap_or_default();
    let val: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
    if !status.is_success() {
        return Err(if text.is_empty() { val.to_string() } else { text });
    }
    Ok(val)
}

pub fn handle_mem(json_mode: bool, action: MemAction) {
    use std::io::Read;
    let (base, token) = read_api_config();
    let http = build_http(&token);

    let result: Result<serde_json::Value, String> = match action {
        MemAction::Search { query, kind, area, scope, limit, rerank } => {
            let mut url = format!("{}/api/mem/search?q={}&limit={}",
                base, urlencoding::encode(&query), limit);
            if let Some(k) = &kind { url.push_str(&format!("&kind={}", k)); }
            if let Some(a) = &area { url.push_str(&format!("&area={}", a)); }
            if let Some(s) = &scope { url.push_str(&format!("&scope={}", s)); }
            if rerank { url.push_str("&rerank=true"); }
            mem_get_request(&http, &url)
        }
        MemAction::Get { id } => {
            mem_get_request(&http, &format!("{}/api/mem/items/{}", base, id))
        }
        MemAction::Delete { id } => {
            let resp = http.delete(format!("{}/api/mem/items/{}", base, id))
                .send().map_err(|e| e.to_string());
            resp.map(|r| serde_json::json!({"deleted": r.status().is_success()}))
        }
        MemAction::History { id } => {
            let val = mem_get_request(&http, &format!("{}/api/mem/items/{}/history", base, id));
            // Pretty-print timeline in human mode; keep raw JSON for --json.
            if !json_mode {
                match &val {
                    Ok(v) => {
                        if let Some(rows) = v.get("history").and_then(|h| h.as_array()) {
                            print_history_timeline(rows);
                            return;
                        }
                    }
                    Err(_) => {}
                }
            }
            val
        }
        MemAction::Taxonomy { workspace } => {
            let mut url = format!("{}/api/mem/taxonomy", base);
            if let Some(ws) = &workspace {
                url.push_str(&format!("?workspace={}", urlencoding::encode(ws)));
            }
            let val = mem_get_request(&http, &url);
            if !json_mode {
                if let Ok(ref v) = val {
                    print_taxonomy(v);
                    return;
                }
            }
            val
        }
        MemAction::Tail { follow, limit } => {
            return run_mem_tail(&http, &base, json_mode, follow, limit);
        }
        MemAction::Update { id, title, summary, area, tags, body, importance } => {
            let tag_list: Option<Vec<String>> = tags.map(|t|
                t.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
            );
            let payload = serde_json::json!({
                "title": title, "summary": summary, "area": area,
                "tags": tag_list, "body": body, "importance": importance,
            });
            let resp = http.patch(format!("{}/api/mem/items/{}", base, id))
                .json(&payload)
                .send()
                .map_err(|e| e.to_string());
            match resp {
                Ok(r) => {
                    let status = r.status();
                    let val: serde_json::Value = r.json().unwrap_or_default();
                    if status.is_success() { Ok(val) } else { Err(val.to_string()) }
                }
                Err(e) => Err(e),
            }
        }
        MemAction::Save { kind, title, body, summary, tags, area, importance, scope, source_path, replace, skip_extract } => {
            // Body: positional arg, else read stdin if piped
            let resolved_body = match body {
                Some(b) => b,
                None => {
                    let mut s = String::new();
                    if std::io::stdin().read_to_string(&mut s).is_err() || s.trim().is_empty() {
                        eprintln!("Error: no body provided (pass as arg or pipe to stdin)");
                        std::process::exit(1);
                    }
                    s
                }
            };
            let resolved_title = title.unwrap_or_else(|| {
                resolved_body.lines().next().unwrap_or("untitled").chars().take(80).collect()
            });
            let split_csv = |raw: Option<String>| -> Vec<String> {
                raw.map(|t| t.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
                    .unwrap_or_default()
            };
            let tag_list = split_csv(tags);
            let payload = serde_json::json!({
                "kind": kind,
                "title": resolved_title,
                "summary": summary,
                "area": area,
                "tags": tag_list,
                "body": resolved_body,
                "importance": importance,
                "scope": scope,
                "source_path": source_path,
                "replace": replace,
                "skip_extract": skip_extract,
            });
            mem_post_request(&http, &format!("{}/api/mem/items", base), &payload)
        }
        MemAction::ExtractPending { limit } => {
            mem_post_request(
                &http,
                &format!("{}/api/mem/extract-pending?limit={}", base, limit),
                &serde_json::json!({}),
            )
        }
        MemAction::Skill { action } => match action {
            MemSkillAction::New { name, area } => {
                let skills_root = dirs::home_dir().unwrap_or_default().join(data_dir_name()).join("skills");
                let folder_path = skills_root.join(&name);
                if folder_path.exists() {
                    eprintln!("Error: skill folder already exists: {}", folder_path.display());
                    std::process::exit(1);
                }
                if let Err(e) = std::fs::create_dir_all(folder_path.join("scripts")) {
                    eprintln!("Error creating folder: {}", e);
                    std::process::exit(1);
                }
                let area_line = area.as_deref().unwrap_or("general");
                let template = format!(
                    "---\ntitle: {name}\narea: {area_line}\ntags: []\n---\n\n# {name}\n\n_One-paragraph summary that becomes the search snippet._\n\n## When to use\n\n…\n\n## Steps\n\n1. …\n2. …\n\n## Gotchas\n\n…\n\n## Scripts\n\n- `./scripts/foo.sh` — describe what it does\n",
                    name = name, area_line = area_line,
                );
                let skill_md = folder_path.join("SKILL.md");
                if let Err(e) = std::fs::write(&skill_md, template) {
                    eprintln!("Error writing SKILL.md: {}", e);
                    std::process::exit(1);
                }
                eprintln!("Created skill folder: {}", folder_path.display());
                eprintln!("Edit: {}", skill_md.display());
                eprintln!("Add scripts to: {}", folder_path.join("scripts").display());
                eprintln!();
                // Register immediately so it appears in mem search right away.
                let body = std::fs::read_to_string(&skill_md).unwrap_or_default();
                let (fm_title, fm_area, _fm_tags) = parse_skill_frontmatter(&body);
                let title = fm_title.unwrap_or(name.clone());
                let area_resolved = area.clone().or(fm_area);
                let summary = format!("Skill scaffold — edit {} to fill in.", skill_md.display());
                let source = format!("file:{}", skill_md.display());
                let mut payload = serde_json::json!({
                    "kind": "skill",
                    "title": title,
                    "body": body,
                    "summary": summary,
                    "source": source,
                    "source_path": source,
                    "tags": ["folder-skill", "scaffold"],
                    "importance": 4.0,
                    "skip_extract": true,
                    "replace": true,
                });
                if let Some(a) = area_resolved {
                    payload["area"] = serde_json::json!(a);
                }
                mem_post_request(&http, &format!("{}/api/mem/items", base), &payload)
            }
            MemSkillAction::Add { folder, area, importance } => {
                let folder_expanded = if let Some(stripped) = folder.strip_prefix("~/") {
                    dirs::home_dir().map(|h| h.join(stripped)).unwrap_or_else(|| std::path::PathBuf::from(&folder))
                } else if folder == "~" {
                    dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("~"))
                } else {
                    std::path::PathBuf::from(&folder)
                };
                let folder_path = folder_expanded;
                let skill_md = folder_path.join("SKILL.md");
                if !skill_md.exists() {
                    eprintln!("Error: SKILL.md not found in {}", folder_path.display());
                    std::process::exit(1);
                }
                let body = match std::fs::read_to_string(&skill_md) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("Error reading SKILL.md: {}", e);
                        std::process::exit(1);
                    }
                };
                // Optional YAML-ish frontmatter parse: between two --- lines at top.
                // Picks `title:`, `area:`, `tags:` (comma list) if present.
                let (fm_title, fm_area, fm_tags) = parse_skill_frontmatter(&body);
                // Title priority: frontmatter → first H1 → folder name.
                let title = fm_title.or_else(|| body.lines()
                    .find(|l| l.trim_start().starts_with("# "))
                    .map(|l| l.trim_start().trim_start_matches("# ").trim().to_string()))
                    .unwrap_or_else(|| folder_path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "skill".into()));
                // Area priority: --area flag → frontmatter → parent folder name (if grandparent is skills/).
                let area_resolved = area.clone().or(fm_area).or_else(|| {
                    let parent = folder_path.parent()?.file_name()?.to_string_lossy().to_string();
                    if parent == "skills" { None } else { Some(parent) }
                });
                // Skip the YAML frontmatter block before picking summary.
                let body_after_fm = strip_skill_frontmatter(&body);
                let summary: String = body_after_fm.lines()
                    .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
                    .next().unwrap_or("").to_string();
                let source = format!("file:{}", skill_md.display());
                let mut tags: Vec<String> = vec!["folder-skill".into()];
                tags.extend(fm_tags);
                let mut payload = serde_json::json!({
                    "kind": "skill",
                    "title": title,
                    "body": body,
                    "summary": summary,
                    "source": source,
                    "source_path": source,
                    "tags": tags,
                    "importance": importance,
                    "skip_extract": true,
                    "replace": true,
                });
                if let Some(a) = area_resolved {
                    payload["area"] = serde_json::json!(a);
                }
                mem_post_request(&http, &format!("{}/api/mem/items", base), &payload)
            }
            MemSkillAction::List => {
                match mem_get_request(&http, &format!("{}/api/mem/tail?limit=500", base)) {
                    Ok(resp) => {
                        if let Some(rows) = resp.get("rows").and_then(|v| v.as_array()) {
                            let skills: Vec<&serde_json::Value> = rows.iter()
                                .filter(|r| r.get("kind").and_then(|k| k.as_str()) == Some("skill"))
                                .collect();
                            Ok(serde_json::json!({"skills": skills}))
                        } else {
                            Ok(resp)
                        }
                    }
                    Err(e) => Err(e),
                }
            }
        },
        MemAction::WakeUp { budget, l0_only, no_l0 } => {
            let mut url = format!("{}/api/mem/wake-up", base);
            let mut sep = '?';
            if let Some(b) = budget {
                url.push_str(&format!("{}budget={}", sep, b));
                sep = '&';
            }
            if l0_only { url.push_str(&format!("{}l0_only=true", sep)); sep = '&'; }
            if no_l0 { url.push_str(&format!("{}no_l0=true", sep)); let _ = sep; }
            mem_get_request(&http, &url)
        }
        MemAction::Stats => {
            mem_get_request(&http, &format!("{}/api/mem/stats", base))
        }
        MemAction::Learn { text, kind, tags, error, feature } => {
            let (resolved_text, resolved_kind) = if let Some(t) = error {
                (t, "error".to_string())
            } else if let Some(t) = feature {
                (t, "feature".to_string())
            } else if let Some(t) = text {
                (t, kind.unwrap_or_else(|| "insight".into()))
            } else {
                let mut s = String::new();
                if std::io::stdin().read_to_string(&mut s).is_err() || s.trim().is_empty() {
                    eprintln!("Error: provide text as arg, --error/--feature, or pipe to stdin");
                    std::process::exit(1);
                }
                (s, kind.unwrap_or_else(|| "insight".into()))
            };
            let tag_list: Vec<String> = tags
                .map(|t| t.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
                .unwrap_or_default();
            mem_post_request(&http, &format!("{}/api/mem/learn", base), &serde_json::json!({
                "text": resolved_text, "kind": resolved_kind, "tags": tag_list,
            }))
        }
        MemAction::LearnList => {
            mem_get_request(&http, &format!("{}/api/mem/search?q=learning&kind=learning&limit=50", base))
        }
        MemAction::LearnPromote { id } => {
            mem_post_request(&http, &format!("{}/api/mem/promote/{}", base, id), &serde_json::json!({}))
        }
        MemAction::Flush { pane, title } => {
            let pane_id = pane
                .or_else(|| std::env::var("CODETERM_PANE_ID").ok())
                .unwrap_or_else(|| "current".to_string());
            mem_post_request(&http, &format!("{}/api/mem/flush", base), &serde_json::json!({
                "pane": pane_id, "title": title,
            }))
        }
        MemAction::Dream { phase } => {
            let url = format!("{}/api/mem/dream?phase={}", base, urlencoding::encode(&phase));
            mem_post_request(&http, &url, &serde_json::json!({}))
        }
        MemAction::Sync { path } => {
            let mut url = format!("{}/api/mem/sync", base);
            if let Some(p) = path {
                url.push_str(&format!("?path={}", urlencoding::encode(&p)));
            }
            mem_post_request(&http, &url, &serde_json::json!({}))
        }
        MemAction::Graph { action } => match action {
            MemGraphAction::Create { id, entity_type, props, scope } => {
                let props_val: serde_json::Value =
                    serde_json::from_str(&props).unwrap_or(serde_json::json!({}));
                let payload = serde_json::json!({
                    "id": id, "type": entity_type, "scope": scope, "properties": props_val,
                });
                mem_post_request(&http, &format!("{}/api/mem/graph/entities", base), &payload)
            }
            MemGraphAction::Relate { from, rel, to, valid_from, valid_to, confidence, scope } => {
                let payload = serde_json::json!({
                    "from": from, "rel": rel, "to": to,
                    "valid_from": valid_from, "valid_to": valid_to,
                    "confidence": confidence, "scope": scope,
                });
                mem_post_request(&http, &format!("{}/api/mem/graph/relate", base), &payload)
            }
            MemGraphAction::Related { id, rel, as_of, include_invalidated } => {
                let mut url = format!("{}/api/mem/graph/related?id={}", base, urlencoding::encode(&id));
                if let Some(r) = &rel { url.push_str(&format!("&rel={}", r)); }
                if let Some(d) = &as_of { url.push_str(&format!("&as_of={}", urlencoding::encode(d))); }
                if include_invalidated { url.push_str("&include_invalidated=true"); }
                mem_get_request(&http, &url)
            }
            MemGraphAction::Invalidate { from, rel, to, ended } => {
                let payload = serde_json::json!({
                    "from": from, "rel": rel, "to": to, "ended": ended,
                });
                mem_post_request(&http, &format!("{}/api/mem/graph/invalidate", base), &payload)
            }
            MemGraphAction::Timeline { entity } => {
                let url = format!("{}/api/mem/graph/timeline?entity={}", base, urlencoding::encode(&entity));
                mem_get_request(&http, &url)
            }
            MemGraphAction::Traverse { from, hops, max_nodes, as_of, include_invalidated } => {
                let mut url = format!("{}/api/mem/graph/traverse?from={}&hops={}&max_nodes={}",
                    base, urlencoding::encode(&from), hops, max_nodes);
                if let Some(d) = &as_of { url.push_str(&format!("&as_of={}", urlencoding::encode(d))); }
                if include_invalidated { url.push_str("&include_invalidated=true"); }
                mem_get_request(&http, &url)
            }
            MemGraphAction::Path { from, to, max_hops, include_invalidated } => {
                let mut url = format!("{}/api/mem/graph/path?from={}&to={}&max_hops={}",
                    base, urlencoding::encode(&from), urlencoding::encode(&to), max_hops);
                if include_invalidated { url.push_str("&include_invalidated=true"); }
                mem_get_request(&http, &url)
            }
            MemGraphAction::Tunnels { include_invalidated } => {
                let mut url = format!("{}/api/mem/graph/tunnels", base);
                if include_invalidated { url.push_str("?include_invalidated=true"); }
                mem_get_request(&http, &url)
            }
            MemGraphAction::Alias { alias, canonical } => {
                let payload = serde_json::json!({ "alias": alias, "canonical": canonical });
                mem_post_request(&http, &format!("{}/api/mem/graph/alias", base), &payload)
            }
        }
        MemAction::Worker { action } => match action {
            MemWorkerAction::Spawn { intent, task, agent, parent } => {
                let payload = serde_json::json!({
                    "intent": intent,
                    "task": task,
                    "agent": agent,
                    "parent": parent,
                });
                mem_post_request(&http, &format!("{}/api/mem/workers", base), &payload)
            }
            MemWorkerAction::List => {
                mem_get_request(&http, &format!("{}/api/mem/workers", base))
            }
            MemWorkerAction::Get { id } => {
                mem_get_request(&http, &format!("{}/api/mem/workers/{}", base, id))
            }
            MemWorkerAction::Progress { text, status } => {
                match std::env::var("CODETERM_WORKER_ID") {
                    Ok(id) => {
                        let payload = serde_json::json!({ "text": text, "status": status });
                        mem_post_request(&http, &format!("{}/api/mem/workers/{}/progress", base, id), &payload)
                    }
                    Err(_) => Err("not in a worker pane: $CODETERM_WORKER_ID is not set".into()),
                }
            }
            MemWorkerAction::Report { summary } => {
                match std::env::var("CODETERM_WORKER_ID") {
                    Ok(id) => {
                        let payload = serde_json::json!({ "summary": summary });
                        mem_post_request(&http, &format!("{}/api/mem/workers/{}/report", base, id), &payload)
                    }
                    Err(_) => Err("not in a worker pane: $CODETERM_WORKER_ID is not set".into()),
                }
            }
            MemWorkerAction::Kill { id } => {
                let resp = http.delete(format!("{}/api/mem/workers/{}", base, id))
                    .send().map_err(|e| e.to_string());
                resp.map(|r| serde_json::json!({"ok": r.status().is_success()}))
            }
        }
        MemAction::RunWorker { .. } => unreachable!("RunWorker is short-circuited in main()"),
        MemAction::Repair { .. } => unreachable!("Repair is short-circuited in main()"),
        MemAction::Secret { action } => {
            return crate::cmd::mem_secret::handle_mem_secret(json_mode, &http, &base, action);
        }
    };

    // Wake-up returns plain text in `text` field — print it directly.
    if let Ok(ref val) = result {
        if let Some(text) = val.get("text").and_then(|v| v.as_str()) {
            println!("{}", text);
            return;
        }
    }
    handle_result(json_mode, result);
}

fn run_mem_tail(
    http: &reqwest::blocking::Client,
    base: &str,
    json_mode: bool,
    follow: bool,
    limit: usize,
) {
    let fetch = |since: Option<&str>| -> Result<Vec<serde_json::Value>, String> {
        let mut url = format!("{}/api/mem/tail?limit={}", base, limit);
        if let Some(s) = since {
            url.push_str(&format!("&since={}", urlencoding::encode(s)));
        }
        let val = mem_get_request(http, &url)?;
        Ok(val
            .get("rows")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default())
    };
    let print_row = |row: &serde_json::Value| {
        let ts = row
            .get("updated_at")
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .split('.')
            .next()
            .unwrap_or("?")
            .replace('T', " ");
        let kind = row.get("kind").and_then(|v| v.as_str()).unwrap_or("?");
        let who = row.get("changed_by").and_then(|v| v.as_str()).unwrap_or("?");
        let title = row.get("title").and_then(|v| v.as_str()).unwrap_or("?");
        let area = row
            .get("area")
            .and_then(|v| v.as_str())
            .unwrap_or("(none)");
        println!(
            "{} | {:<12} | {:<15} | {:<60} | {}",
            ts,
            kind,
            who,
            title.chars().take(60).collect::<String>(),
            area
        );
    };

    match fetch(None) {
        Ok(rows) => {
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&rows).unwrap());
            } else if rows.is_empty() {
                println!("(no recent writes)");
            } else {
                // Oldest first so that follow-mode output streams naturally.
                for row in rows.iter().rev() {
                    print_row(row);
                }
            }
            if !follow {
                return;
            }
            // In follow mode we remember the newest updated_at we've printed
            // and poll with `since` every 500ms.
            let mut last_seen: Option<String> = rows
                .iter()
                .filter_map(|r| r.get("updated_at").and_then(|v| v.as_str()).map(String::from))
                .max();
            loop {
                std::thread::sleep(std::time::Duration::from_millis(500));
                let next = match fetch(last_seen.as_deref()) {
                    Ok(rows) => rows,
                    Err(e) => {
                        eprintln!("tail fetch: {}", e);
                        std::thread::sleep(std::time::Duration::from_secs(2));
                        continue;
                    }
                };
                if !next.is_empty() {
                    for row in next.iter().rev() {
                        print_row(row);
                    }
                    if let Some(newest) = next
                        .iter()
                        .filter_map(|r| r.get("updated_at").and_then(|v| v.as_str()).map(String::from))
                        .max()
                    {
                        last_seen = Some(newest);
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}
