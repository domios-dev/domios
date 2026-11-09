use serde::Serialize;
use std::collections::HashSet;

use super::{PlanError, PlanStore};
use crate::plan::{Section, Task};

mod inputs;
pub(super) use inputs::{SectionLinkInput, SectionLinkSource};

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
pub(super) struct SectionCandidate {
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

fn candidate_fields(
    id: &str,
    title: &str,
    handle: Option<&str>,
    tags: &[String],
) -> SectionCandidate {
    let mut keys = HashSet::new();
    add_key(&mut keys, id);
    add_key(&mut keys, title);
    if let Some(handle) = handle {
        add_key(&mut keys, handle);
    }
    for tag in tags {
        add_key(&mut keys, tag);
    }
    SectionCandidate {
        id: id.into(),
        title: title.into(),
        keys,
    }
}

fn collect_sections(sections: &[Section], out: &mut Vec<SectionCandidate>) {
    for section in sections {
        out.push(candidate_fields(
            &section.id,
            &section.title,
            section.handle.as_deref(),
            &section.tags,
        ));
        collect_sections(&section.subsections, out);
    }
}

fn section_candidates(source: &impl SectionLinkSource) -> Vec<SectionCandidate> {
    let mut out = Vec::new();
    source.collect_candidates(&mut out);
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
    name: &str,
    handle: Option<&str>,
    group: Option<&str>,
) -> Option<(&'a SectionCandidate, String)> {
    if let Some(group) = group.filter(|group| !group.trim().is_empty()) {
        if let Some(hit) = unique_from_text(sections, group) {
            return Some((hit.0, format!("matched group '{group}'")));
        }
    }
    if let Some(handle) = handle.filter(|handle| !handle.trim().is_empty()) {
        if let Some(hit) = unique_from_text(sections, handle) {
            return Some((hit.0, format!("matched handle '{handle}'")));
        }
    }
    let name = normalize(name);
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

pub(super) fn resolve_selectors(
    plan: &impl SectionLinkSource,
    selectors: &[String],
) -> Vec<String> {
    let candidates = section_candidates(plan);
    selectors
        .iter()
        .filter_map(|selector| {
            unique_from_text(&candidates, selector).map(|(section, _)| section.id.clone())
        })
        .collect()
}

pub(super) fn auto_section_ids_for_task(plan: &impl SectionLinkSource, task: &Task) -> Vec<String> {
    if !task.section_ids.is_empty() {
        return task.section_ids.clone();
    }
    let sections = section_candidates(plan);
    suggest_for_task(
        &sections,
        &task.name,
        task.handle.as_deref(),
        task.group.as_deref(),
    )
    .map(|(section, _)| vec![section.id.clone()])
    .or_else(|| (sections.len() == 1).then(|| vec![sections[0].id.clone()]))
    .unwrap_or_default()
}

impl PlanStore {
    pub fn resolve_task_section_ids(
        &self,
        plan_id: &str,
        selectors: &[String],
    ) -> Result<Vec<String>, PlanError> {
        let plan = self.database.section_link_projection(plan_id, false)?;
        Ok(resolve_selectors(&plan, selectors))
    }

    pub fn suggested_section_for_task(
        &self,
        plan_id: &str,
        task: &Task,
    ) -> Result<Option<SectionLinkSuggestion>, PlanError> {
        let plan = self.database.section_link_projection(plan_id, false)?;
        Ok(suggest_section(&plan, task))
    }

    pub fn link_task_sections(
        &self,
        plan_id: &str,
        task_id: &str,
        selectors: &[String],
        now_ms: u64,
    ) -> Result<Vec<String>, PlanError> {
        let turn = self.mutation_writer.enter();
        let (linked, receipt) = self
            .database
            .link_task_section_rows(plan_id, task_id, selectors, now_ms)?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(linked)
    }

    pub fn relink_unlinked_tasks(
        &self,
        plan_id: &str,
        apply: bool,
        now_ms: u64,
    ) -> Result<SectionRelinkReport, PlanError> {
        if !apply {
            let plan = self.database.section_link_projection(plan_id, true)?;
            return Ok(section_link_preview(&plan));
        }

        let turn = self.mutation_writer.enter();
        let (report, receipt) = self.database.relink_task_section_rows(plan_id, now_ms)?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(report)
    }
}

pub(super) fn suggest_section(
    plan: &impl SectionLinkSource,
    task: &Task,
) -> Option<SectionLinkSuggestion> {
    let sections = section_candidates(plan);
    suggest_for_task(
        &sections,
        &task.name,
        task.handle.as_deref(),
        task.group.as_deref(),
    )
    .map(|(section, reason)| SectionLinkSuggestion {
        task_id: task.id.clone(),
        task_name: task.name.clone(),
        section_id: section.id.clone(),
        section_title: section.title.clone(),
        reason,
    })
}

pub(super) fn section_link_preview(plan: &impl SectionLinkSource) -> SectionRelinkReport {
    let sections = section_candidates(plan);
    let mut suggestions = vec![];
    let mut unlinked_count = 0;
    for task in plan.unlinked_tasks() {
        unlinked_count += 1;
        if let Some((section, reason)) = suggest_for_task(
            &sections,
            &task.name,
            task.handle.as_deref(),
            task.group.as_deref(),
        ) {
            suggestions.push(SectionLinkSuggestion {
                task_id: task.id.clone(),
                task_name: task.name.clone(),
                section_id: section.id.clone(),
                section_title: section.title.clone(),
                reason,
            });
        }
    }
    SectionRelinkReport {
        applied: vec![],
        suggestions,
        unlinked_count,
    }
}

#[cfg(test)]
mod tests;
