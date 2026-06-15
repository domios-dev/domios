use std::path::PathBuf;

use super::{mem_get_request, mem_post_request};
use crate::{data_dir_name, MemSkillAction};

pub(crate) fn handle_mem_skill(
    http: &reqwest::blocking::Client,
    base: &str,
    action: MemSkillAction,
) -> Result<serde_json::Value, String> {
    match action {
        MemSkillAction::New { name, area } => {
            let skills_root = dirs::home_dir()
                .unwrap_or_default()
                .join(data_dir_name())
                .join("skills");
            let folder_path = skills_root.join(&name);
            if folder_path.exists() {
                eprintln!(
                    "Error: skill folder already exists: {}",
                    folder_path.display()
                );
                std::process::exit(1);
            }
            if let Err(e) = std::fs::create_dir_all(folder_path.join("scripts")) {
                eprintln!("Error creating folder: {}", e);
                std::process::exit(1);
            }
            let area_line = area.as_deref().unwrap_or("general");
            let template = format!(
                "---\ntitle: {name}\narea: {area_line}\ntags: []\n---\n\n# {name}\n\n_One-paragraph summary that becomes the search snippet._\n\n## When to use\n\n…\n\n## Steps\n\n1. …\n2. …\n\n## Gotchas\n\n…\n\n## Scripts\n\n- `./scripts/foo.sh` — describe what it does\n",
                name = name,
                area_line = area_line,
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
            let body = std::fs::read_to_string(&skill_md).unwrap_or_default();
            let (fm_title, fm_area, _fm_tags) = parse_skill_frontmatter(&body);
            let title = fm_title.unwrap_or(name.clone());
            let area_resolved = area.or(fm_area);
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
            mem_post_request(http, &format!("{}/api/mem/items", base), &payload)
        }
        MemSkillAction::Add {
            folder,
            area,
            importance,
        } => {
            let folder_expanded = if let Some(stripped) = folder.strip_prefix("~/") {
                dirs::home_dir()
                    .map(|h| h.join(stripped))
                    .unwrap_or_else(|| PathBuf::from(&folder))
            } else if folder == "~" {
                dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"))
            } else {
                PathBuf::from(&folder)
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
            let (fm_title, fm_area, fm_tags) = parse_skill_frontmatter(&body);
            let title = fm_title
                .or_else(|| {
                    body.lines()
                        .find(|l| l.trim_start().starts_with("# "))
                        .map(|l| l.trim_start().trim_start_matches("# ").trim().to_string())
                })
                .unwrap_or_else(|| {
                    folder_path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "skill".into())
                });
            let area_resolved = area.or(fm_area).or_else(|| {
                let parent = folder_path
                    .parent()?
                    .file_name()?
                    .to_string_lossy()
                    .to_string();
                if parent == "skills" {
                    None
                } else {
                    Some(parent)
                }
            });
            let body_after_fm = strip_skill_frontmatter(&body);
            let summary: String = body_after_fm
                .lines()
                .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
                .next()
                .unwrap_or("")
                .to_string();
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
            mem_post_request(http, &format!("{}/api/mem/items", base), &payload)
        }
        MemSkillAction::List => {
            match mem_get_request(http, &format!("{}/api/mem/tail?limit=500", base)) {
                Ok(resp) => {
                    if let Some(rows) = resp.get("rows").and_then(|v| v.as_array()) {
                        let skills: Vec<&serde_json::Value> = rows
                            .iter()
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
    }
}

fn strip_skill_frontmatter(body: &str) -> &str {
    let mut lines = body.lines();
    if lines.next().map(|l| l.trim()) != Some("---") {
        return body;
    }
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
        if t == "---" {
            break;
        }
        if let Some(v) = t.strip_prefix("title:") {
            title = Some(v.trim().trim_matches('"').to_string());
        } else if let Some(v) = t.strip_prefix("area:") {
            area = Some(v.trim().trim_matches('"').to_string());
        } else if let Some(v) = t.strip_prefix("tags:") {
            tags = v
                .trim()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .split(',')
                .map(|s| s.trim().trim_matches('"').to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }
    (title, area, tags)
}
