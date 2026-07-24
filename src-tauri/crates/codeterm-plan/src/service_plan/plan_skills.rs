use std::path::Path;

use serde::{Deserialize, Serialize};

use super::paths::{plan_dir, plan_sidecar_root};
use crate::service_plan::PlanError;
use crate::service_plan::PlanStore;

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

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct NewPlanSkill {
    pub slug: String,
    pub title: Option<String>,
    pub area: Option<String>,
    pub check: Option<String>,
    pub rationale: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CreatedPlanSkill {
    pub slug: String,
    pub title: String,
    pub check: Option<String>,
    pub skill_md: String,
    pub scripts: String,
    pub ledger_seq: u64,
    pub verdict: String,
    pub rationale: String,
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
                !t.is_empty() && !t.starts_with('#')
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

fn slugify(raw: &str) -> String {
    let mut out = String::new();
    let mut last_dash = true;
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

fn scaffold(slug: &str, title: &str, area: &str, check: Option<&str>) -> String {
    let check_fm = check
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!("check: {}\n", value.trim()))
        .unwrap_or_default();
    let check_section = check
        .filter(|value| !value.trim().is_empty())
        .map(|value| format!("\n## Check\n\nVerify with: `{}`\n", value.trim()))
        .unwrap_or_default();
    format!(
        "---\ntitle: {title}\narea: {area}\ntags: [plan-skill]\n{check_fm}---\n\n# {title}\n\n_One-paragraph summary that becomes the search snippet._\n\n## When to use\n\n…\n\n## Steps\n\n1. …\n2. …\n\n## Gotchas\n\n…{check_section}\n\n## Scripts\n\n- `./scripts/{slug}.sh` — describe what it does\n",
    )
}

fn plan_dir_for(store: &PlanStore, plan_id: &str) -> Result<std::path::PathBuf, PlanError> {
    let plan = store.get(plan_id).ok_or(PlanError::NotFound)?;
    let root = plan_sidecar_root(&plan);
    plan_dir(root, &plan.status, &plan.id).map_err(|e| PlanError::Io(e.to_string()))
}

fn io_error(context: &str, error: impl std::fmt::Display) -> PlanError {
    PlanError::Io(format!("{context}: {error}"))
}

fn remove_path(path: &Path) {
    if path.is_dir() {
        let _ = std::fs::remove_dir_all(path);
    } else {
        let _ = std::fs::remove_file(path);
    }
}

fn restore_ledger(ledger: &Path, backup: &Path) {
    if backup.is_file() {
        let _ = std::fs::remove_file(ledger);
        let _ = std::fs::rename(backup, ledger);
    }
}

fn commit_skill(
    staged_skill: &Path,
    final_skill: &Path,
    staged_ledger: &Path,
    ledger: &Path,
    ledger_backup: &Path,
) -> Result<(), PlanError> {
    if ledger.is_file() {
        std::fs::rename(ledger, ledger_backup)
            .map_err(|error| io_error("stage existing plan skill ledger", error))?;
    }
    if let Err(error) = std::fs::rename(staged_skill, final_skill) {
        restore_ledger(ledger, ledger_backup);
        return Err(io_error("commit plan skill directory", error));
    }
    if let Err(error) = std::fs::rename(staged_ledger, ledger) {
        remove_path(final_skill);
        restore_ledger(ledger, ledger_backup);
        return Err(io_error("commit plan skill ledger", error));
    }
    remove_path(ledger_backup);
    Ok(())
}

pub fn create_plan_skill(
    store: &PlanStore,
    plan_id: &str,
    request: NewPlanSkill,
) -> Result<CreatedPlanSkill, PlanError> {
    let _turn = store.mutation_writer.enter();
    let plan_dir = plan_dir_for(store, plan_id)?;
    let slug = slugify(&request.slug);
    if slug.is_empty() {
        return Err(PlanError::InvalidPlanSkillSlug);
    }
    let root = skills_root(&plan_dir);
    std::fs::create_dir_all(&root).map_err(|error| io_error("create plan skills root", error))?;
    let final_skill = root.join(&slug);
    if final_skill.exists() {
        return Err(PlanError::PlanSkillAlreadyExists { slug });
    }

    let staged_skill = root.join(format!(".{slug}.pending"));
    let staged_ledger = root.join(format!(".ledger.{slug}.pending"));
    let ledger = root.join("ledger.jsonl");
    let ledger_backup = root.join(format!(".ledger.{slug}.backup"));
    for path in [&staged_skill, &staged_ledger, &ledger_backup] {
        remove_path(path);
    }

    let title = request.title.unwrap_or_else(|| titleize(&slug));
    let area = request.area.unwrap_or_else(|| "general".to_string());
    let check = request
        .check
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let rationale = request
        .rationale
        .unwrap_or_else(|| "created".to_string())
        .trim()
        .replace(['\n', '\r'], " ");
    if rationale.is_empty() {
        return Err(PlanError::Io(
            "plan skill rationale must be a non-empty one-line note".to_string(),
        ));
    }

    let result = (|| {
        std::fs::create_dir_all(staged_skill.join("scripts"))
            .map_err(|error| io_error("stage plan skill directory", error))?;
        std::fs::write(
            staged_skill.join("SKILL.md"),
            scaffold(&slug, &title, &area, check.as_deref()),
        )
        .map_err(|error| io_error("stage plan skill body", error))?;

        let mut ledger_body = match std::fs::read_to_string(&ledger) {
            Ok(body) => body,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(io_error("read plan skill ledger", error)),
        };
        let ledger_seq = read_ledger(&plan_dir)
            .last()
            .map(|entry| entry.seq + 1)
            .unwrap_or(1);
        let line = serde_json::to_string(&serde_json::json!({
            "seq": ledger_seq,
            "kind": "skill",
            "slug": slug,
            "verdict": "keep",
            "rationale": rationale,
        }))
        .map_err(|error| io_error("serialize plan skill ledger", error))?;
        if !ledger_body.is_empty() && !ledger_body.ends_with('\n') {
            ledger_body.push('\n');
        }
        ledger_body.push_str(&line);
        ledger_body.push('\n');
        std::fs::write(&staged_ledger, ledger_body)
            .map_err(|error| io_error("stage plan skill ledger", error))?;
        commit_skill(
            &staged_skill,
            &final_skill,
            &staged_ledger,
            &ledger,
            &ledger_backup,
        )?;
        Ok(ledger_seq)
    })();

    let ledger_seq = match result {
        Ok(seq) => seq,
        Err(error) => {
            remove_path(&staged_skill);
            remove_path(&staged_ledger);
            restore_ledger(&ledger, &ledger_backup);
            return Err(error);
        }
    };
    Ok(CreatedPlanSkill {
        slug,
        title,
        check,
        skill_md: final_skill.join("SKILL.md").display().to_string(),
        scripts: final_skill.join("scripts").display().to_string(),
        ledger_seq,
        verdict: "keep".to_string(),
        rationale,
    })
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
    let slug = slugify(slug);
    if slug.is_empty() {
        return Err(PlanError::InvalidPlanSkillSlug);
    }
    let skill_md = plan_dir.join("skills").join(&slug).join("SKILL.md");
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
        title: title.unwrap_or_else(|| titleize(&slug)),
        slug,
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
        crate::service_plan::paths::mark_test_project_root(tmp.path());
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

    #[test]
    fn create_skill_scaffolds_and_ledgers_once() {
        let tmp = tempfile::tempdir().unwrap();
        crate::service_plan::paths::mark_test_project_root(tmp.path());
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
        let created = create_plan_skill(
            &store,
            &plan.id,
            NewPlanSkill {
                slug: "Deploy Safely".into(),
                title: Some("Safe Deploy".into()),
                area: Some("release".into()),
                check: Some("./scripts/check.sh".into()),
                rationale: Some("accepted".into()),
            },
        )
        .unwrap();
        assert_eq!(created.slug, "deploy-safely");
        assert_eq!(created.title, "Safe Deploy");
        assert_eq!(created.check.as_deref(), Some("./scripts/check.sh"));
        assert_eq!(created.ledger_seq, 1);
        assert_eq!(created.verdict, "keep");

        let detail = show_plan_skill(&store, &plan.id, &created.slug).unwrap();
        assert!(detail.body.contains("area: release"));
        assert!(detail.body.contains("check: ./scripts/check.sh"));
        assert_eq!(detail.ledger.len(), 1);
        assert_eq!(detail.ledger[0].seq, 1);
        assert_eq!(detail.ledger[0].rationale, "accepted");
        assert!(Path::new(&created.skill_md).is_file());
        assert!(Path::new(&created.scripts).is_dir());

        let duplicate = create_plan_skill(
            &store,
            &plan.id,
            NewPlanSkill {
                slug: "Deploy Safely".into(),
                title: None,
                area: None,
                check: None,
                rationale: None,
            },
        )
        .unwrap_err();
        assert!(matches!(
            duplicate,
            PlanError::PlanSkillAlreadyExists { .. }
        ));
        assert_eq!(
            show_plan_skill(&store, &plan.id, &created.slug)
                .unwrap()
                .ledger
                .len(),
            1
        );
    }

    #[test]
    fn create_skill_write_failure_leaves_no_partial_skill_or_ledger() {
        let tmp = tempfile::tempdir().unwrap();
        crate::service_plan::paths::mark_test_project_root(tmp.path());
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
        std::fs::write(dir.join("skills"), "blocked").unwrap();

        let error = create_plan_skill(
            &store,
            &plan.id,
            NewPlanSkill {
                slug: "deploy".into(),
                title: None,
                area: None,
                check: None,
                rationale: None,
            },
        )
        .unwrap_err();
        assert!(matches!(error, PlanError::Io(_)));
        assert!(dir.join("skills").is_file());
        assert!(!dir.join("skills").join("deploy").exists());
        assert!(!dir.join("skills").join("ledger.jsonl").exists());
    }
}
