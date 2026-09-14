use serde::Serialize;
use std::collections::HashSet;

use super::{PlanError, PlanStore};
use crate::plan::{Plan, Section, Task};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SectionLinkSuggestion {
    pub task_id: String,
    pub task_name: String,
    pub section_id: String,
    pub section_title: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SectionRelinkReport {
    pub applied: Vec<SectionLinkSuggestion>,
    pub suggestions: Vec<SectionLinkSuggestion>,
    pub unlinked_count: usize,
}

#[derive(Debug, Clone)]
struct SectionCandidate {
    id: String,
    title: String,
    keys: HashSet<String>,
}

fn normalize(text: &str) -> String {
    text.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn compact_key(text: &str) -> String {
    normalize(text).replace(' ', "")
}

fn add_key(keys: &mut HashSet<String>, value: &str) {
    let key = normalize(value);
    if !key.is_empty() {
        keys.insert(key.clone());
        keys.insert(compact_key(&key));
    }
}

fn collect_sections(sections: &[Section], out: &mut Vec<SectionCandidate>) {
    for section in sections {
        let mut keys = HashSet::new();
        add_key(&mut keys, &section.id);
        add_key(&mut keys, &section.title);
        if let Some(handle) = &section.handle {
            add_key(&mut keys, handle);
        }
        for tag in &section.tags {
            add_key(&mut keys, tag);
        }
        out.push(SectionCandidate {
            id: section.id.clone(),
            title: section.title.clone(),
            keys,
        });
        collect_sections(&section.subsections, out);
    }
}

fn section_candidates(plan: &Plan) -> Vec<SectionCandidate> {
    let mut out = vec![];
    collect_sections(&plan.sections, &mut out);
    out
}

fn unique_by_key<'a>(sections: &'a [SectionCandidate], key: &str) -> Option<&'a SectionCandidate> {
    if key.is_empty() {
        return None;
    }
    let mut matches = sections
        .iter()
        .filter(|section| section.keys.contains(key))
        .collect::<Vec<_>>();
    matches.dedup_by_key(|section| section.id.as_str());
    if matches.len() == 1 {
        Some(matches[0])
    } else {
        None
    }
}

fn unique_from_text<'a>(
    sections: &'a [SectionCandidate],
    text: &str,
) -> Option<(&'a SectionCandidate, String)> {
    let normalized = normalize(text);
    let compact = compact_key(text);
    for key in [normalized.as_str(), compact.as_str()] {
        if let Some(section) = unique_by_key(sections, key) {
            return Some((section, format!("matched '{text}'")));
        }
    }
    None
}

fn suggest_for_task<'a>(
    sections: &'a [SectionCandidate],
    task: &Task,
) -> Option<(&'a SectionCandidate, String)> {
    if let Some(group) = task
        .group
        .as_deref()
        .filter(|group| !group.trim().is_empty())
    {
        if let Some(hit) = unique_from_text(sections, group) {
            return Some((hit.0, format!("matched group '{group}'")));
        }
    }
    if let Some(handle) = task
        .handle
        .as_deref()
        .filter(|handle| !handle.trim().is_empty())
    {
        if let Some(hit) = unique_from_text(sections, handle) {
            return Some((hit.0, format!("matched handle '{handle}'")));
        }
    }
    let name = normalize(&task.name);
    let matches = sections
        .iter()
        .filter(|section| {
            section
                .keys
                .iter()
                .any(|key| name.contains(key) && key.len() >= 3)
        })
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        Some((matches[0], "matched task name".into()))
    } else {
        None
    }
}

fn resolve_selectors(plan: &Plan, selectors: &[String]) -> Vec<String> {
    let candidates = section_candidates(plan);
    selectors
        .iter()
        .filter_map(|selector| {
            unique_from_text(&candidates, selector).map(|(section, _)| section.id.clone())
        })
        .collect()
}

pub(super) fn auto_section_ids_for_task(plan: &Plan, task: &Task) -> Vec<String> {
    if !task.section_ids.is_empty() {
        return task.section_ids.clone();
    }
    let sections = section_candidates(plan);
    suggest_for_task(&sections, task)
        .map(|(section, _)| vec![section.id.clone()])
        .unwrap_or_default()
}

impl PlanStore {
    pub fn resolve_task_section_ids(
        &self,
        plan_id: &str,
        selectors: &[String],
    ) -> Result<Vec<String>, PlanError> {
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        Ok(resolve_selectors(&plan, selectors))
    }

    pub fn suggested_section_for_task(
        &self,
        plan_id: &str,
        task: &Task,
    ) -> Result<Option<SectionLinkSuggestion>, PlanError> {
        let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
        let sections = section_candidates(&plan);
        Ok(
            suggest_for_task(&sections, task).map(|(section, reason)| SectionLinkSuggestion {
                task_id: task.id.clone(),
                task_name: task.name.clone(),
                section_id: section.id.clone(),
                section_title: section.title.clone(),
                reason,
            }),
        )
    }

    pub fn link_task_sections(
        &self,
        plan_id: &str,
        task_id: &str,
        selectors: &[String],
        now_ms: u64,
    ) -> Result<Vec<String>, PlanError> {
        let mut linked = vec![];
        self.with_plan(plan_id, now_ms, |plan| {
            let resolved = resolve_selectors(plan, selectors);
            let task = crate::plan::task_key::find_task_mut(&mut plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            for section_id in resolved {
                if !task.section_ids.iter().any(|id| id == &section_id) {
                    task.section_ids.push(section_id.clone());
                }
                linked.push(section_id);
            }
            task.updated_at = now_ms;
            Ok(())
        })?;
        Ok(linked)
    }

    pub fn relink_unlinked_tasks(
        &self,
        plan_id: &str,
        apply: bool,
        now_ms: u64,
    ) -> Result<SectionRelinkReport, PlanError> {
        if !apply {
            let plan = self.get(plan_id).ok_or(PlanError::NotFound)?;
            let sections = section_candidates(&plan);
            let mut suggestions = vec![];
            let mut unlinked_count = 0;
            for task in plan.tasks.iter().filter(|task| task.section_ids.is_empty()) {
                unlinked_count += 1;
                if let Some((section, reason)) = suggest_for_task(&sections, task) {
                    suggestions.push(SectionLinkSuggestion {
                        task_id: task.id.clone(),
                        task_name: task.name.clone(),
                        section_id: section.id.clone(),
                        section_title: section.title.clone(),
                        reason,
                    });
                }
            }
            return Ok(SectionRelinkReport {
                applied: vec![],
                suggestions,
                unlinked_count,
            });
        }
        let mut report = SectionRelinkReport {
            applied: vec![],
            suggestions: vec![],
            unlinked_count: 0,
        };
        self.with_plan(plan_id, now_ms, |plan| {
            let sections = section_candidates(plan);
            for task in plan
                .tasks
                .iter_mut()
                .filter(|task| task.section_ids.is_empty())
            {
                report.unlinked_count += 1;
                let Some((section, reason)) = suggest_for_task(&sections, task) else {
                    continue;
                };
                let suggestion = SectionLinkSuggestion {
                    task_id: task.id.clone(),
                    task_name: task.name.clone(),
                    section_id: section.id.clone(),
                    section_title: section.title.clone(),
                    reason,
                };
                if apply {
                    task.section_ids.push(section.id.clone());
                    task.updated_at = now_ms;
                    report.applied.push(suggestion);
                } else {
                    report.suggestions.push(suggestion);
                }
            }
            Ok(())
        })?;
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{OrchestratorRef, Plan};
    use crate::service_plan::NewTask;

    fn store_with_backend_section() -> (PlanStore, String) {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "Plan".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("owner"),
        );
        let plan_id = plan.id.clone();
        store.save(&mut plan, 1).unwrap();
        store
            .add_section(&plan_id, "Backend", "", None, &[], None, Some("backend"), 2)
            .unwrap();
        (store, plan_id)
    }

    #[test]
    fn add_task_auto_links_by_group() {
        let (store, plan_id) = store_with_backend_section();
        let task_id = store
            .add_task(
                &plan_id,
                NewTask {
                    name: "Implement route".into(),
                    kind: "implement".into(),
                    group: Some("backend".into()),
                    competition: None,
                    ..Default::default()
                },
                3,
            )
            .unwrap();
        let plan = store.get(&plan_id).unwrap();
        let task = plan.tasks.iter().find(|task| task.id == task_id).unwrap();
        assert_eq!(task.section_ids.len(), 1);
    }

    #[test]
    fn relink_preview_does_not_mutate_and_apply_links() {
        let (store, plan_id) = store_with_backend_section();
        let task_id = store
            .add_task(
                &plan_id,
                NewTask {
                    name: "Backend follow-up".into(),
                    kind: "implement".into(),
                    competition: None,
                    ..Default::default()
                },
                3,
            )
            .unwrap();
        store
            .update_task(
                &plan_id,
                &task_id,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                Some(vec![]),
                None,
                None,
                &[],
                None,
                None,
                4,
            )
            .unwrap();

        let preview = store.relink_unlinked_tasks(&plan_id, false, 5).unwrap();
        assert_eq!(preview.suggestions.len(), 1);
        assert!(store.get(&plan_id).unwrap().tasks[0].section_ids.is_empty());

        let applied = store.relink_unlinked_tasks(&plan_id, true, 6).unwrap();
        assert_eq!(applied.applied.len(), 1);
        assert_eq!(store.get(&plan_id).unwrap().tasks[0].section_ids.len(), 1);
    }
}
