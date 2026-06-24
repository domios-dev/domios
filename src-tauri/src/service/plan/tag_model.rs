use crate::plan::{Plan, Section, Task};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TagValueStats {
    pub value: String,
    pub label: Option<String>,
    pub color: Option<String>,
    pub sections: usize,
    pub tasks: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TagFacetStats {
    pub key: String,
    pub label: Option<String>,
    pub color_lane: bool,
    pub values: Vec<TagValueStats>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TagTopicStats {
    pub tag: String,
    pub sections: usize,
    pub tasks: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TagDuplicateWarning {
    pub a: String,
    pub b: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TagReport {
    pub facets: Vec<TagFacetStats>,
    pub topics: Vec<TagTopicStats>,
    pub duplicate_warnings: Vec<TagDuplicateWarning>,
}

pub fn normalize_plan(plan: &mut Plan) {
    let mut used = HashSet::new();
    normalize_sections(&mut plan.sections, &mut used);
    for task in &mut plan.tasks {
        let seed = task
            .handle
            .clone()
            .unwrap_or_else(|| handle_seed(&task.tags, &task.name));
        task.handle = Some(unique_handle(seed, &mut used));
    }
}

pub fn normalize_sections_for_spec(sections: &mut [Section]) {
    let mut used = HashSet::new();
    normalize_sections(sections, &mut used);
}

pub fn find_task<'a>(plan: &'a Plan, selector: &str) -> Option<&'a Task> {
    plan.tasks
        .iter()
        .find(|task| task.id == selector || task.handle.as_deref() == Some(selector))
}

pub fn section_handle(section: &Section) -> &str {
    section.handle.as_deref().unwrap_or(&section.id)
}

pub fn task_handle(task: &Task) -> &str {
    task.handle.as_deref().unwrap_or(&task.id)
}

pub fn apply_tags(tags: &mut Vec<String>, add: &[String], primary: Option<&str>) {
    for tag in add {
        if !tags.iter().any(|existing| existing == tag) {
            tags.push(tag.clone());
        }
    }
    if let Some(primary) = primary {
        tags.retain(|tag| tag != primary);
        tags.insert(0, primary.to_string());
    }
}

pub fn tag_report(plan: &Plan) -> TagReport {
    let mut section_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut task_counts: BTreeMap<String, usize> = BTreeMap::new();
    for section in flatten_sections(&plan.sections) {
        for tag in &section.tags {
            *section_counts.entry(tag.clone()).or_default() += 1;
        }
    }
    for task in &plan.tasks {
        for tag in &task.tags {
            *task_counts.entry(tag.clone()).or_default() += 1;
        }
    }

    let facets = plan
        .tag_facets
        .iter()
        .map(|facet| {
            let values = facet
                .values
                .iter()
                .map(|value| {
                    let tag = format!("{}:{}", facet.key, value.v);
                    TagValueStats {
                        value: value.v.clone(),
                        label: value.label.clone(),
                        color: value.color.clone(),
                        sections: section_counts.get(&tag).copied().unwrap_or(0),
                        tasks: task_counts.get(&tag).copied().unwrap_or(0),
                    }
                })
                .collect();
            TagFacetStats {
                key: facet.key.clone(),
                label: facet.label.clone(),
                color_lane: facet.color_lane,
                values,
            }
        })
        .collect();

    let declared: BTreeSet<String> = plan
        .tag_facets
        .iter()
        .flat_map(|facet| {
            facet
                .values
                .iter()
                .map(|value| format!("{}:{}", facet.key, value.v))
        })
        .collect();
    let all_tags: BTreeSet<String> = section_counts
        .keys()
        .chain(task_counts.keys())
        .cloned()
        .collect();
    let topics = all_tags
        .iter()
        .filter(|tag| !declared.contains(*tag) && !tag.contains(':'))
        .map(|tag| TagTopicStats {
            tag: tag.clone(),
            sections: section_counts.get(tag).copied().unwrap_or(0),
            tasks: task_counts.get(tag).copied().unwrap_or(0),
        })
        .collect();

    TagReport {
        facets,
        topics,
        duplicate_warnings: duplicate_warnings(&all_tags),
    }
}

fn normalize_sections(sections: &mut [Section], used: &mut HashSet<String>) {
    for section in sections {
        let seed = section
            .handle
            .clone()
            .unwrap_or_else(|| handle_seed(&section.tags, &section.title));
        section.handle = Some(unique_handle(seed, used));
        normalize_sections(&mut section.subsections, used);
    }
}

fn handle_seed(tags: &[String], title: &str) -> String {
    let primary = tags
        .iter()
        .find_map(|tag| tag.split_once(':').map(|(_, v)| v));
    let topic = tags
        .iter()
        .find(|tag| !tag.contains(':'))
        .map(String::as_str)
        .or_else(|| title.split_whitespace().next())
        .unwrap_or("item");
    match primary {
        Some(primary) => format!("{}-{}", slug(primary), slug(topic)),
        None => slug(title),
    }
}

fn unique_handle(seed: String, used: &mut HashSet<String>) -> String {
    let seed = if seed.is_empty() { "item".into() } else { seed };
    if used.insert(seed.clone()) {
        return seed;
    }
    for n in 2.. {
        let candidate = format!("{seed}-{n}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!()
}

fn slug(raw: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in raw.chars().flat_map(|ch| ch.to_lowercase()) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    out.trim_matches('-').to_string()
}

fn flatten_sections(sections: &[Section]) -> Vec<&Section> {
    let mut out = vec![];
    for section in sections {
        out.push(section);
        out.extend(flatten_sections(&section.subsections));
    }
    out
}

fn duplicate_warnings(tags: &BTreeSet<String>) -> Vec<TagDuplicateWarning> {
    let mut by_squashed: HashMap<String, String> = HashMap::new();
    let mut out = vec![];
    for tag in tags {
        let key: String = tag.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        if let Some(prev) = by_squashed.insert(key, tag.clone()) {
            if prev != *tag {
                out.push(TagDuplicateWarning {
                    a: prev,
                    b: tag.clone(),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{OrchestratorRef, Plan};

    #[test]
    fn generated_handles_use_primary_facet_and_topic() {
        let mut plan = Plan::new("P".into(), "/repo".into(), OrchestratorRef::default());
        plan.tasks.push(crate::plan::Task {
            id: "task_1".into(),
            name: "Pan Zoom".into(),
            kind: "implement".into(),
            model: None,
            model_rationale: None,
            acceptance_criteria: vec![],
            test_plan: None,
            correctness: None,
            status: crate::plan::TaskStatus::Pending,
            status_label: None,
            priority: Default::default(),
            created_at: 0,
            updated_at: 0,
            status_changed_at: 0,
            handle: None,
            tags: vec!["area:nav".into(), "pan-zoom".into()],
            section_ids: vec![],
            body: String::new(),
            intent: None,
            parent_task_id: None,
            assigned_pane_id: None,
            taken_by_role: None,
            peer: None,
            group: None,
            canvas: Default::default(),
            milestones: vec![],
        });
        normalize_plan(&mut plan);
        assert_eq!(plan.tasks[0].handle.as_deref(), Some("nav-pan-zoom"));
    }
}
