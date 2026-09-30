use serde::Serialize;

use super::{PlanError, PlanStore};
use crate::plan::{Plan, Section};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct IngestedSection {
    pub section_id: String,
    pub handle: String,
    pub title: String,
    pub created: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlanIngestResult {
    pub sections: Vec<IngestedSection>,
}

#[derive(Debug, Clone, PartialEq)]
struct MarkdownSection {
    level: usize,
    title: String,
    body: String,
    handle: String,
    children: Vec<MarkdownSection>,
}

fn gen_id() -> String {
    crate::plan::ids::short_id("sec")
}

fn handle_from_title(title: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in title.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    let trimmed = out.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "section".into()
    } else {
        trimmed
    }
}

fn heading(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim_start();
    let level = trimmed.chars().take_while(|ch| *ch == '#').count();
    if !(1..=6).contains(&level) || !trimmed[level..].starts_with(' ') {
        return None;
    }
    let title = trimmed[level..]
        .trim()
        .trim_end_matches('#')
        .trim()
        .to_string();
    (!title.is_empty()).then_some((level, title))
}

fn unique_handle(title: &str, used: &mut Vec<String>) -> String {
    let base = handle_from_title(title);
    if !used.iter().any(|item| item == &base) {
        used.push(base.clone());
        return base;
    }
    for n in 2.. {
        let candidate = format!("{base}-{n}");
        if !used.iter().any(|item| item == &candidate) {
            used.push(candidate.clone());
            return candidate;
        }
    }
    unreachable!()
}

fn children_at_path<'a>(
    roots: &'a mut Vec<MarkdownSection>,
    path: &[usize],
) -> &'a mut Vec<MarkdownSection> {
    let mut siblings = roots;
    for &idx in path {
        siblings = &mut siblings[idx].children;
    }
    siblings
}

fn section_mut_at<'a>(
    roots: &'a mut [MarkdownSection],
    path: &[usize],
) -> Option<&'a mut MarkdownSection> {
    let mut current = roots.get_mut(*path.first()?)?;
    for &idx in &path[1..] {
        current = current.children.get_mut(idx)?;
    }
    Some(current)
}

fn parse_markdown_sections(markdown: &str) -> Vec<MarkdownSection> {
    let mut roots = Vec::new();
    let mut stack: Vec<(usize, Vec<usize>)> = Vec::new();
    let mut used = Vec::new();
    for line in markdown.lines() {
        if let Some((level, title)) = heading(line) {
            while stack.last().is_some_and(|(existing, _)| *existing >= level) {
                stack.pop();
            }
            let parent_path = stack
                .last()
                .map(|(_, path)| path.clone())
                .unwrap_or_default();
            let section = MarkdownSection {
                level,
                handle: unique_handle(&title, &mut used),
                title,
                body: String::new(),
                children: Vec::new(),
            };
            let siblings = children_at_path(&mut roots, &parent_path);
            siblings.push(section);
            let mut path = parent_path;
            path.push(siblings.len() - 1);
            stack.push((level, path));
            continue;
        }
        if let Some((_, path)) = stack.last() {
            let Some(current) = section_mut_at(&mut roots, path) else {
                continue;
            };
            if !current.body.is_empty() {
                current.body.push('\n');
            }
            current.body.push_str(line);
        }
    }
    if roots.len() == 1 && roots[0].level == 1 && roots[0].body.trim().is_empty() {
        roots.remove(0).children
    } else {
        roots
            .into_iter()
            .filter(|section| {
                section.level > 1 || !section.body.trim().is_empty() || !section.children.is_empty()
            })
            .collect()
    }
}

fn find_matching_section<'a>(
    sections: &'a mut [Section],
    incoming: &MarkdownSection,
) -> Option<&'a mut Section> {
    sections.iter_mut().find(|section| {
        section.handle.as_deref() == Some(incoming.handle.as_str())
            || section.title.trim() == incoming.title.trim()
    })
}

fn upsert_sections(
    target: &mut Vec<Section>,
    incoming: &[MarkdownSection],
    result: &mut Vec<IngestedSection>,
) {
    for section in incoming {
        if let Some(existing) = find_matching_section(target, section) {
            existing.title = section.title.clone();
            existing.handle = Some(section.handle.clone());
            existing.body = section.body.trim().to_string();
            let section_id = existing.id.clone();
            upsert_sections(&mut existing.subsections, &section.children, result);
            result.push(IngestedSection {
                section_id,
                handle: section.handle.clone(),
                title: section.title.clone(),
                created: false,
            });
        } else {
            let mut new_section = Section {
                id: gen_id(),
                title: section.title.clone(),
                handle: Some(section.handle.clone()),
                tags: vec![],
                body: section.body.trim().to_string(),
                subsections: vec![],
                canvas: Default::default(),
                attachments: vec![],
            };
            upsert_sections(&mut new_section.subsections, &section.children, result);
            result.push(IngestedSection {
                section_id: new_section.id.clone(),
                handle: section.handle.clone(),
                title: section.title.clone(),
                created: true,
            });
            target.push(new_section);
        }
    }
}

pub(super) fn ingest_sections_into_plan(plan: &mut Plan, markdown: &str) -> PlanIngestResult {
    let parsed = parse_markdown_sections(markdown);
    let mut sections = Vec::new();
    upsert_sections(&mut plan.sections, &parsed, &mut sections);
    PlanIngestResult { sections }
}

impl PlanStore {
    pub fn ingest_plan_sections(
        &self,
        plan_id: &str,
        markdown: &str,
        now_ms: u64,
    ) -> Result<PlanIngestResult, PlanError> {
        self.with_plan(plan_id, now_ms, |plan| {
            Ok(ingest_sections_into_plan(plan, markdown))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{OrchestratorRef, Plan};

    #[test]
    fn parser_maps_headings_to_section_bodies() {
        let parsed =
            parse_markdown_sections("# Plan\n\n## Backend\nDo A\n\n### API\nDo B\n\n## UI\nDo C");

        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].title, "Backend");
        assert_eq!(parsed[0].body, "Do A\n");
        assert_eq!(parsed[0].children[0].title, "API");
        assert_eq!(parsed[1].body, "Do C");
    }

    #[test]
    fn ingest_upserts_existing_section_by_handle() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "Plan".into(),
            dir.path().display().to_string(),
            OrchestratorRef::default(),
        );
        plan.id = "pl_ingest".into();
        plan.sections.push(Section {
            id: "sec_existing".into(),
            title: "Draft backend".into(),
            handle: Some("backend".into()),
            tags: vec![],
            body: String::new(),
            subsections: vec![],
            canvas: Default::default(),
            attachments: vec![],
        });
        store.save(&mut plan, 1).unwrap();

        let result = store
            .ingest_plan_sections("pl_ingest", "## Backend\nImplement the service.", 2)
            .unwrap();

        assert_eq!(result.sections.len(), 1);
        assert!(!result.sections[0].created);
        let plan = store.get("pl_ingest").unwrap();
        assert_eq!(plan.sections[0].id, "sec_existing");
        assert_eq!(plan.sections[0].title, "Backend");
        assert_eq!(plan.sections[0].body, "Implement the service.");
    }
}
