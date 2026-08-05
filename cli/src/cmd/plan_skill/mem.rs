use std::path::Path;

use super::store::skill_md_path;

pub fn mem_payload(plan_id: &str, slug: &str, skill_md: &Path, body: &str) -> serde_json::Value {
    let (title, area, mut tags) = parse_frontmatter(body);
    let title = title.unwrap_or_else(|| slug.to_string());
    tags.push("plan-skill".to_string());
    let source = format!("file:{}", skill_md.display());
    let mut payload = serde_json::json!({
        "kind": "skill",
        "title": title,
        "body": body,
        "summary": format!("Plan skill '{slug}' for plan {plan_id}."),
        "source": source,
        "source_path": source,
        "plan_id": plan_id,
        "tags": tags,
        "importance": 4.0,
        "skip_extract": true,
        "replace": true,
    });
    if let Some(a) = area {
        payload["area"] = serde_json::json!(a);
    }
    payload
}

// on-disk skill store/ledger writes that already succeeded.
pub fn index_skill(plan_id: &str, plan_dir: &Path, slug: &str) -> Result<(), String> {
    let skill_md = skill_md_path(plan_dir, slug);
    let body = std::fs::read_to_string(&skill_md).map_err(|e| e.to_string())?;
    let payload = mem_payload(plan_id, slug, &skill_md, &body);
    let (base, _) = crate::read_api_config();
    let http = crate::build_local_api().map_err(|e| e.to_string())?;
    let resp = http
        .post(format!("{base}/api/mem/items"))
        .json(&payload)
        .send()
        .map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        Ok(())
    } else {
        Err(format!("mem index HTTP {}", resp.status()))
    }
}

fn parse_frontmatter(body: &str) -> (Option<String>, Option<String>, Vec<String>) {
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
    fn payload_is_plan_scoped() {
        let body = "---\ntitle: Deploy\narea: infra\ntags: [ci]\n---\n\n# Deploy\n";
        let payload = mem_payload("plan_x", "deploy", Path::new("/p/SKILL.md"), body);
        assert_eq!(payload["kind"], "skill");
        assert_eq!(payload["plan_id"], "plan_x");
        assert_eq!(payload["title"], "Deploy");
        assert_eq!(payload["area"], "infra");
        let tags = payload["tags"].as_array().unwrap();
        assert!(tags.iter().any(|t| t == "plan-skill"));
        assert!(tags.iter().any(|t| t == "ci"));
    }

    #[test]
    fn payload_falls_back_to_slug_title() {
        let payload = mem_payload("p", "my-skill", Path::new("/p/SKILL.md"), "no frontmatter");
        assert_eq!(payload["title"], "my-skill");
        assert!(payload.get("area").is_none());
    }
}
