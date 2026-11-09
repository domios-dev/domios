use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::service_plan::PlanError;
use crate::service_plan::PlanStore;

#[derive(Debug, Clone, Serialize, PartialEq, ts_rs::TS)]
pub struct PlanSkillLedgerEntry {
    pub seq: u64,
    pub slug: String,
    pub verdict: String,
    pub rationale: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, ts_rs::TS)]
pub struct PlanSkillSummary {
    pub slug: String,
    pub title: String,
    pub purpose: String,
    pub check: Option<String>,
    pub verdict: String,
    pub content_digest: String,
    /// On-disk skill folder, so a caller whose `show` is refused still has a
    /// path to read the SKILL.md directly.
    pub skill_dir: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, ts_rs::TS)]
pub struct PlanSkillDetail {
    pub slug: String,
    pub title: String,
    pub body: String,
    pub ledger: Vec<PlanSkillLedgerEntry>,
}

pub fn plan_skill_manifest(
    store: &PlanStore,
    plan_id: &str,
) -> Result<Vec<crate::plan::PlanSkillManifestEntry>, PlanError> {
    let plan_dir = plan_dir_for(store, plan_id)?;
    plan_skill_manifest_from_dir(&plan_dir)
}

pub(super) fn plan_skill_manifest_for_plan(
    plan: &crate::plan::Plan,
) -> Result<Vec<crate::plan::PlanSkillManifestEntry>, PlanError> {
    let plan_dir = super::paths::plan_state_dir(
        super::paths::plan_sidecar_root(plan),
        &plan.status,
        &plan.id,
    )
    .map_err(|error| PlanError::Io(error.to_string()))?;
    plan_skill_manifest_from_dir(&plan_dir)
}

/// A ledgered skill directory paired with its true latest ledger verdict.
/// The single read of disk + ledger that every plan-skill read surface
/// builds on: the AI-context manifest (keep-only) and the human/audit
/// `list_plan_skills` (every ledgered skill, real verdict) both derive from
/// this instead of each re-deciding what "on disk" means.
struct PlanSkillFileEntry {
    slug: String,
    verdict: String,
    title: String,
    purpose: String,
    check: Option<String>,
    content_digest: String,
}

// A directory with SKILL.md but no ledger entry was never committed —
// `create_plan_skill` always writes both atomically — so it is skipped here.
fn plan_skill_file_entries(plan_dir: &Path) -> Result<Vec<PlanSkillFileEntry>, PlanError> {
    let latest_verdicts = read_ledger(plan_dir).into_iter().fold(
        std::collections::HashMap::new(),
        |mut verdicts, entry| {
            verdicts.insert(entry.slug, entry.verdict);
            verdicts
        },
    );
    let mut entries = Vec::new();
    for slug in list_slugs(plan_dir) {
        let Some(verdict) = latest_verdicts.get(&slug).cloned() else {
            continue;
        };
        let path = plan_dir.join("skills").join(&slug).join("SKILL.md");
        let body = std::fs::read(&path).map_err(|error| io_error("read plan skill body", error))?;
        let text = std::str::from_utf8(&body)
            .map_err(|error| io_error("decode plan skill body", error))?;
        let (title, purpose, check) = parse_frontmatter(text);
        entries.push(PlanSkillFileEntry {
            slug: slug.clone(),
            verdict,
            title: title.unwrap_or_else(|| titleize(&slug)),
            purpose: purpose.unwrap_or_default(),
            check,
            content_digest: format!("sha256:{:x}", Sha256::digest(&body)),
        });
    }
    Ok(entries)
}

// AI-context surface only: still filtered to the currently-kept roster, so a
// discarded/superseded skill is never injected into a prompt as if active.
pub(super) fn plan_skill_manifest_from_dir(
    plan_dir: &Path,
) -> Result<Vec<crate::plan::PlanSkillManifestEntry>, PlanError> {
    Ok(plan_skill_file_entries(plan_dir)?
        .into_iter()
        .filter(|entry| entry.verdict == "keep")
        .map(|entry| crate::plan::PlanSkillManifestEntry {
            slug: entry.slug,
            title: entry.title,
            purpose: entry.purpose,
            content_digest: entry.content_digest,
        })
        .collect())
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
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|slug| !slug.starts_with('.'))
        .filter(|slug| root.join(slug).join("SKILL.md").is_file())
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

const SCAFFOLD_SUMMARY_PLACEHOLDER: &str = "One-paragraph summary that becomes the search snippet.";

struct ParsedSkillDocument {
    metadata: Option<serde_yaml::Value>,
    raw_metadata: String,
    body: String,
}

fn parse_skill_document(md: &str) -> Option<ParsedSkillDocument> {
    let trimmed = md.trim_start();
    let Some(rest) = trimmed.strip_prefix("---") else {
        return Some(ParsedSkillDocument {
            metadata: None,
            raw_metadata: String::new(),
            body: trimmed.to_string(),
        });
    };
    let rest = rest.trim_start_matches(['\r', '\n']);
    let mut lines = rest.lines();
    let mut frontmatter = Vec::new();
    let mut body = None;
    for line in lines.by_ref() {
        if line.trim() == "---" {
            body = Some(lines.collect::<Vec<_>>().join("\n"));
            break;
        }
        frontmatter.push(line);
    }
    let body = body?;
    let raw_metadata = frontmatter.join("\n");
    let metadata = serde_yaml::from_str(&raw_metadata).ok();
    Some(ParsedSkillDocument {
        metadata,
        raw_metadata,
        body,
    })
}

fn metadata_string(document: &ParsedSkillDocument, key: &str) -> Option<String> {
    document
        .metadata
        .as_ref()
        .and_then(|metadata| {
            metadata
                .as_mapping()?
                .get(serde_yaml::Value::String(key.to_string()))?
                .as_str()
                .map(str::to_string)
        })
        .or_else(|| {
            document.raw_metadata.lines().find_map(|line| {
                let (candidate, value) = line.split_once(':')?;
                (candidate.trim() == key)
                    .then(|| value.trim().to_string())
                    .filter(|value| !value.is_empty())
            })
        })
}

fn normalize_purpose(value: &str) -> Option<String> {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let compact = compact
        .trim_matches(|ch| matches!(ch, '_' | '*' | '`'))
        .trim();
    if compact.is_empty() || compact == SCAFFOLD_SUMMARY_PLACEHOLDER {
        None
    } else {
        Some(compact.to_string())
    }
}

fn first_summary_paragraph(body: &str) -> Option<String> {
    for block in body.split("\n\n") {
        let line = block
            .lines()
            .find(|line| {
                let t = line.trim();
                !t.is_empty() && !t.starts_with('#')
            })
            .unwrap_or("");
        if let Some(purpose) = normalize_purpose(line) {
            return Some(purpose);
        }
    }
    None
}

fn parse_frontmatter(md: &str) -> (Option<String>, Option<String>, Option<String>) {
    let Some(document) = parse_skill_document(md) else {
        return (None, None, None);
    };
    let title = metadata_string(&document, "title");
    let check = metadata_string(&document, "check")
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let purpose = ["description", "purpose", "summary"]
        .into_iter()
        .find_map(|key| metadata_string(&document, key))
        .and_then(|value| normalize_purpose(&value))
        .or_else(|| first_summary_paragraph(&document.body));
    (title, purpose, check)
}

#[cfg(test)]
pub(super) fn persisted_skill_purpose(md: &str) -> Option<String> {
    parse_frontmatter(md).1
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
    store
        .database
        .storage_metadata(plan_id)?
        .state_dir()
        .map_err(|e| PlanError::Io(e.to_string()))
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

// The ledger keeps every committed skill with its current verdict so superseding one does not erase its history.
pub fn list_plan_skills(
    store: &PlanStore,
    plan_id: &str,
) -> Result<Vec<PlanSkillSummary>, PlanError> {
    let plan_dir = plan_dir_for(store, plan_id)?;
    Ok(plan_skill_file_entries(&plan_dir)?
        .into_iter()
        .map(|entry| PlanSkillSummary {
            skill_dir: plan_dir
                .join("skills")
                .join(&entry.slug)
                .display()
                .to_string(),
            slug: entry.slug,
            title: entry.title,
            purpose: entry.purpose,
            check: entry.check,
            verdict: entry.verdict,
            content_digest: entry.content_digest,
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
mod tests;
