use std::path::PathBuf;

use crate::client::SocketClient;
use crate::MemSkillAction;

struct SkillSlug(String);

impl SkillSlug {
    fn parse(value: String) -> Result<Self, String> {
        let bytes = value.as_bytes();
        let valid = bytes
            .first()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            && bytes.len() <= 64
            && bytes.iter().skip(1).all(|b| {
                b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(*b, b'.' | b'_' | b'-')
            });
        let base = value.split('.').next().unwrap_or("").to_ascii_uppercase();
        let device = matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            || ["COM", "LPT"].iter().any(|prefix| {
                base.strip_prefix(prefix)
                    .is_some_and(|n| matches!(n.as_bytes(), [b'1'..=b'9']))
            });
        if value.is_empty() {
            return Err("skill slug must not be empty".into());
        }
        if !valid {
            return Err("skill slug must match [a-z0-9][a-z0-9._-]{0,63} with no path separators or dot-only segments".into());
        }
        if device {
            return Err("skill slug must not be a Windows device name".into());
        }
        Ok(Self(value))
    }
}

fn skill_folder(root: &std::path::Path, slug: &SkillSlug) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("canonicalize skills root {}: {e}", root.display()))?;
    let path = root.join(&slug.0);
    let resolved = if path.exists() {
        path.canonicalize()
            .map_err(|e| format!("canonicalize skill folder: {e}"))?
    } else {
        path.clone()
    };
    if !resolved.starts_with(&root) || resolved == root {
        return Err("skill slug resolves outside skills root".into());
    }
    Ok(path)
}

pub(crate) fn handle_mem_skill(
    client: &mut SocketClient,
    action: MemSkillAction,
) -> Result<serde_json::Value, String> {
    match action {
        MemSkillAction::New { name, area } => {
            let slug = SkillSlug::parse(name.clone())?;
            let skills_root = crate::data_root().join("skills");
            std::fs::create_dir_all(&skills_root)
                .map_err(|e| format!("create skills root: {e}"))?;
            let folder_path = skill_folder(&skills_root, &slug)?;
            if folder_path.exists() {
                crate::cmd::print::fail(
                    1,
                    format!("skill folder already exists: {}", folder_path.display()),
                );
            }
            if let Err(e) = std::fs::create_dir_all(folder_path.join("scripts")) {
                crate::cmd::print::fail(1, format!("creating folder failed: {}", e));
            }
            let area_line = area.as_deref().unwrap_or("general");
            let template = format!(
                "---\ntitle: {name}\narea: {area_line}\ntags: []\n---\n\n# {name}\n\n_One-paragraph summary that becomes the search snippet._\n\n## When to use\n\n…\n\n## Steps\n\n1. …\n2. …\n\n## Gotchas\n\n…\n\n## Scripts\n\n- `./scripts/foo.sh` — describe what it does\n",
                name = name,
                area_line = area_line,
            );
            let skill_md = folder_path.join("SKILL.md");
            if let Err(e) = std::fs::write(&skill_md, template) {
                crate::cmd::print::fail(1, format!("writing SKILL.md failed: {}", e));
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
            client.call("mem.save", Some(payload))
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
                crate::cmd::print::fail(
                    1,
                    format!("SKILL.md not found in {}", folder_path.display()),
                );
            }
            let body = match std::fs::read_to_string(&skill_md) {
                Ok(s) => s,
                Err(e) => {
                    crate::cmd::print::fail(1, format!("reading SKILL.md failed: {}", e));
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
                .find(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
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
            client.call("mem.save", Some(payload))
        }
        MemSkillAction::List => {
            match client.call(
                "mem.tail",
                Some(serde_json::json!({ "since": None::<&str>, "limit": 500 })),
            ) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_slug_rejects_traversal_and_empty() {
        for value in [
            "",
            "../archivist-escape",
            "Bad Name/../x",
            "CON",
            "nul.md",
            "LPT9",
        ] {
            assert!(
                SkillSlug::parse(value.into()).is_err(),
                "accepted {value:?}"
            );
        }
        assert!(SkillSlug::parse("x".repeat(65)).is_err());
    }

    #[test]
    fn valid_skill_slug_resolves_under_skills_root() {
        let root = std::env::temp_dir().join(format!("skills-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let slug = SkillSlug::parse("release-notes_2.v1".into()).unwrap();
        let folder = skill_folder(&root, &slug).unwrap();
        assert_eq!(
            folder,
            root.canonicalize().unwrap().join("release-notes_2.v1")
        );
        assert!(folder.starts_with(root.canonicalize().unwrap()));
        let _ = std::fs::remove_dir_all(root);
    }
}
