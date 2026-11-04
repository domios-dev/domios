use crate::plan::{Plan, Section, TagFacet, Task};
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
    crate::plan::task_key::find_task(&plan.tasks, selector)
}

pub fn section_handle(section: &Section) -> &str {
    section.handle.as_deref().unwrap_or(&section.id)
}

pub fn task_handle(task: &Task) -> &str {
    task.handle.as_deref().unwrap_or(&task.id)
}

pub fn preview_handle(tags: &[String], title: &str) -> String {
    handle_seed(tags, title)
}

pub fn apply_tags(tags: &mut Vec<String>, add: &[String], primary: Option<&str>) {
    if let Some(primary) = primary {
        if let Some(key) = facet_key(primary) {
            clear_facet_key(tags, key);
        }
        tags.retain(|tag| tag != primary);
        tags.insert(0, primary.to_string());
    }
    for tag in add {
        if primary == Some(tag.as_str()) {
            continue;
        }
        if let Some(key) = facet_key(tag) {
            clear_facet_key(tags, key);
        }
        if !tags.iter().any(|existing| existing == tag) {
            tags.push(tag.clone());
        }
    }
}

pub fn inherit_parent_tags(parent: &[String], child: &[String]) -> Vec<String> {
    let mut tags = parent.to_vec();
    apply_tags(&mut tags, child, None);
    tags
}

fn facet_key(tag: &str) -> Option<&str> {
    let (key, value) = tag.split_once(':')?;
    if key.is_empty() || value.is_empty() {
        return None;
    }
    Some(key)
}

fn clear_facet_key(tags: &mut Vec<String>, key: &str) {
    tags.retain(|tag| facet_key(tag) != Some(key));
}

#[derive(Default)]
pub struct TagReportInput {
    pub facets: Vec<TagFacet>,
    pub section_counts: BTreeMap<String, usize>,
    pub task_counts: BTreeMap<String, usize>,
}

impl From<&Plan> for TagReportInput {
    fn from(plan: &Plan) -> Self {
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

        Self {
            facets: plan.tag_facets.clone(),
            section_counts,
            task_counts,
        }
    }
}

pub fn tag_report(input: impl Into<TagReportInput>) -> TagReport {
    let TagReportInput {
        facets: tag_facets,
        section_counts,
        task_counts,
    } = input.into();
    let facets = tag_facets
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

    let declared: BTreeSet<String> = tag_facets
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

pub(in crate::service_plan) fn normalize_sections(
    sections: &mut [Section],
    used: &mut HashSet<String>,
) {
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

pub(in crate::service_plan) fn unique_handle(seed: String, used: &mut HashSet<String>) -> String {
    // Re-slug even stored handles: legacy pre-cap handles blow past OS path
    // limits (Windows MAX_PATH) every time the task file is rewritten.
    let seed = if seed.is_empty() {
        "item".into()
    } else {
        slug(&seed)
    };
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
    // Cap length: handles feed filenames (tasks/<handle>.md, milestones/<handle>-<id>.md);
    // an uncapped slug of a long title blows past the OS 255-byte name limit (os error 63).
    const MAX: usize = 72;
    let mut out = String::new();
    let mut dash = false;
    for ch in raw.chars().flat_map(|ch| ch.to_lowercase()) {
        if out.len() >= MAX {
            break;
        }
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
mod tests;
