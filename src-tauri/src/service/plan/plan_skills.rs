use std::path::Path;

use serde::Serialize;

use super::paths::{plan_dir, plan_sidecar_root};
use crate::service::plan::PlanError;
use crate::service::plan::PlanStore;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlanSkillLedgerEntry {
    pub seq: u64,
    pub slug: String,
    pub verdict: String,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlanSkillSummary {
    pub slug: String,
    pub title: String,
    pub purpose: String,
    pub check: Option<String>,
    pub verdict: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlanSkillDetail {
    pub slug: String,
    pub title: String,
    pub body: String,
    pub ledger: Vec<PlanSkillLedgerEntry>,
}

fn skills_root(plan_dir: &Path) -> std::path::PathBuf {
    plan_dir.join("skills")
}

fn list_slugs(plan_dir: &Path) -> Vec<String> {
    let root = skills_root(plan_dir);
    let Ok(entries) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut slugs: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join("SKILL.md").is_file())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    slugs.sort();
    slugs
}

fn read_ledger(plan_dir: &Path) -> Vec<PlanSkillLedgerEntry> {
    let path = skills_root(plan_dir).join("ledger.jsonl");
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    raw.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let value: serde_json::Value = serde_json::from_str(line).ok()?;
            Some(PlanSkillLedgerEntry {
                seq: value.get("seq")?.as_u64()?,
                slug: value.get("slug")?.as_str()?.to_string(),
                verdict: value.get("verdict")?.as_str()?.to_string(),
                rationale: value.get("rationale")?.as_str()?.to_string(),
            })
        })
        .collect()
}

fn parse_frontmatter(md: &str) -> (Option<String>, Option<String>, Option<String>) {
    let trimmed = md.trim_start();
    let Some(rest) = trimmed.strip_prefix("---") else {
        return (None, None, None);
    };
    let rest = rest.trim_start_matches(['\r', '\n']);
    let mut lines = rest.lines();
    let mut frontmatter = Vec::new();
    let mut body = String::new();
    for line in lines.by_ref() {
        if line.trim() == "---" {
            body = lines.collect::<Vec<_>>().join("\n");
            break;
        }
        frontmatter.push(line);
    }
    if body.is_empty() && !rest.lines().any(|line| line.trim() == "---") {
        return (None, None, None);
    }
    let mut title = None;
    let mut check = None;
    for line in frontmatter {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "title" => title = Some(value.to_string()),
            "check" => check = Some(value.to_string()),
            _ => {}
        }
    }
    let purpose = first_summary_paragraph(&body);
    (title, Some(purpose), check.filter(|c| !c.is_empty()))
}

fn first_summary_paragraph(body: &str) -> String {
    for block in body.split("\n\n") {
        let line = block
            .lines()
            .find(|line| {
                let t = line.trim();
                !t.is_empty() && !t.starts_with('#') && !t.starts_with('_')
            })
            .unwrap_or("");
        let t = line.trim();
        if !t.is_empty() {
            return t.trim_matches('_').to_string();
        }
    }
    String::new()
}

fn titleize(slug: &str) -> String {
    slug.split('-')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn plan_dir_for(store: &PlanStore, plan_id: &str) -> Result<std::path::PathBuf, PlanError> {
    let plan = store.get(plan_id).ok_or(PlanError::NotFound)?;
    let root = plan_sidecar_root(&plan);
    plan_dir(root, &plan.status, &plan.id).map_err(|e| PlanError::Io(e.to_string()))
}

pub fn list_plan_skills(
    store: &PlanStore,
    plan_id: &str,
) -> Result<Vec<PlanSkillSummary>, PlanError> {
    let plan_dir = plan_dir_for(store, plan_id)?;
    let ledger = read_ledger(&plan_dir);
    Ok(list_slugs(&plan_dir)
        .into_iter()
        .map(|slug| {
            let md = std::fs::read_to_string(plan_dir.join("skills").join(&slug).join("SKILL.md"))
                .unwrap_or_default();
            let (title, purpose, check) = parse_frontmatter(&md);
            let verdict = ledger
                .iter()
                .rev()
                .find(|entry| entry.slug == slug)
                .map(|entry| entry.verdict.clone())
                .unwrap_or_else(|| "-".to_string());
            PlanSkillSummary {
                slug: slug.clone(),
                title: title.unwrap_or_else(|| titleize(&slug)),
                purpose: purpose.unwrap_or_default(),
                check,
                verdict,
            }
        })
        .collect())
}

pub fn show_plan_skill(
    store: &PlanStore,
    plan_id: &str,
    slug: &str,
) -> Result<PlanSkillDetail, PlanError> {
    let plan_dir = plan_dir_for(store, plan_id)?;
    let skill_md = plan_dir.join("skills").join(slug).join("SKILL.md");
    if !skill_md.is_file() {
        return Err(PlanError::NotFound);
    }
    let raw = std::fs::read_to_string(&skill_md).map_err(|e| PlanError::Io(e.to_string()))?;
    let (title, _, _) = parse_frontmatter(&raw);
    let ledger: Vec<PlanSkillLedgerEntry> = read_ledger(&plan_dir)
        .into_iter()
        .filter(|entry| entry.slug == slug)
        .collect();
    Ok(PlanSkillDetail {
        slug: slug.to_string(),
        title: title.unwrap_or_else(|| titleize(slug)),
        body: raw,
        ledger,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{OrchestratorRef, PlanStatus};

    fn write_skill(plan_dir: &Path, slug: &str, md: &str) {
        let dir = plan_dir.join("skills").join(slug);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), md).unwrap();
    }

    #[test]
    fn parse_frontmatter_extracts_title_check_and_purpose() {
        let md =
            "---\ntitle: Deploy\ncheck: ./scripts/deploy.sh\n---\n\n_Ship it fast._\n\n## Steps\n";
        let (title, purpose, check) = parse_frontmatter(md);
        assert_eq!(title.as_deref(), Some("Deploy"));
        assert_eq!(purpose.as_deref(), Some("Ship it fast."));
        assert_eq!(check.as_deref(), Some("./scripts/deploy.sh"));
    }

    #[test]
    fn list_and_show_skills_from_sidecar() {
        let tmp = tempfile::tempdir().unwrap();
        crate::service::plan::paths::mark_test_project_root(tmp.path());
        let store = PlanStore::new_for_test(tmp.path());
        let plan = store
            .create_plan(
                "Skills".into(),
                tmp.path().display().to_string(),
                OrchestratorRef::from_pane("orch"),
                vec![],
                1,
            )
            .unwrap();
        let dir = plan_dir(tmp.path(), &PlanStatus::Active, &plan.id).unwrap();
        write_skill(
            &dir,
            "deploy",
            "---\ntitle: Deploy\ncheck: echo ok\n---\n\nDeploy staging safely.\n",
        );
        let listed = list_plan_skills(&store, &plan.id).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].slug, "deploy");
        assert_eq!(listed[0].title, "Deploy");
        assert!(listed[0].purpose.contains("Deploy staging"));
        assert_eq!(listed[0].check.as_deref(), Some("echo ok"));

        let detail = show_plan_skill(&store, &plan.id, "deploy").unwrap();
        assert!(detail.body.contains("Deploy staging"));
    }
}
