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
mod tests {
    use super::*;
    use crate::plan::{OrchestratorRef, Plan};
    use crate::service_plan::{NewTask, PlanStore};

    #[test]
    fn apply_tags_replaces_same_facet_key() {
        let mut tags = vec!["area:nav".into(), "pan-zoom".into()];
        apply_tags(&mut tags, &["area:ui".into()], Some("area:ui"));
        assert_eq!(tags, vec!["area:ui", "pan-zoom"]);
    }

    #[test]
    fn apply_tags_primary_only_swaps_facet_value() {
        let mut tags = vec!["area:nav".into(), "pan-zoom".into()];
        apply_tags(&mut tags, &[], Some("area:layout"));
        assert_eq!(tags, vec!["area:layout", "pan-zoom"]);
    }

    #[test]
    fn preview_handle_matches_normalized_seed() {
        let tags = vec!["area:nav".into(), "pan-zoom".into()];
        assert_eq!(preview_handle(&tags, "Pan Zoom"), "nav-pan-zoom");
    }

    #[test]
    fn slug_is_capped_so_filenames_stay_under_os_limit() {
        let long = "PERF backend profiling found plugin manager snapshot IPC takes seconds and freezes the whole UI during pan and zoom which is terrible".repeat(2);
        let s = slug(&long);
        assert!(s.len() <= 72, "slug must cap length, got {}", s.len());
        assert!(!s.ends_with('-'), "no trailing dash");
        assert_eq!(slug("Pan Zoom"), "pan-zoom");
    }

    #[test]
    fn normalize_caps_legacy_stored_handles() {
        let mut plan = Plan::new("P".into(), "/repo".into(), OrchestratorRef::default());
        plan.tasks.push(crate::plan::Task {
            id: "task_1".into(),
            handle: Some("legacy-uncapped-handle-".repeat(10)),
            ..task_fixture()
        });
        normalize_plan(&mut plan);
        let handle = plan.tasks[0].handle.as_deref().unwrap();
        assert!(
            handle.len() <= 72,
            "stored handle must be re-capped, got {}",
            handle.len()
        );
    }

    #[test]
    fn generated_handles_use_primary_facet_and_topic() {
        let mut plan = Plan::new("P".into(), "/repo".into(), OrchestratorRef::default());
        plan.tasks.push(task_fixture());
        normalize_plan(&mut plan);
        assert_eq!(plan.tasks[0].handle.as_deref(), Some("nav-pan-zoom"));
    }

    #[test]
    fn renaming_task_keeps_existing_handle() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "Handles".into(),
            dir.path().display().to_string(),
            OrchestratorRef::default(),
        );
        store.save(&mut plan, 1).unwrap();
        let task_id = store
            .add_task(
                &plan.id,
                NewTask {
                    name: "original title".into(),
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        let before = store.show_task(&task_id).unwrap().task.handle.unwrap();

        store
            .update_task(
                &plan.id,
                &task_id,
                Some("renamed completely different title"),
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
                None,
                None,
                &[],
                None,
                None,
                3,
            )
            .unwrap();

        let after = store.show_task(&task_id).unwrap().task.handle.unwrap();
        assert_eq!(after, before);
    }

    fn task_fixture() -> crate::plan::Task {
        crate::plan::Task {
            id: "task_1".into(),
            name: "Pan Zoom".into(),
            kind: "implement".into(),
            model: None,
            model_rationale: None,
            acceptance_criteria: vec![],
            external_refs: vec![],
            test_plan: None,
            qa_plan: None,
            correctness: None,
            status: crate::plan::TaskStatus::Pending,
            review_stage: Default::default(),
            qa_state: None,
            review_round: 0,
            review_oracle: None,
            review_evidence: vec![],
            key: None,
            review_override: None,
            review_audit: vec![],
            status_label: None,
            priority: Default::default(),
            created_at: 0,
            updated_at: 0,
            status_changed_at: 0,
            handle: None,
            tags: vec!["area:nav".into(), "pan-zoom".into()],
            section_ids: vec![],
            body: String::new(),
            role_guidance: None,
            executor_role: None,
            competition: None,
            intent: None,
            execution_intent: Default::default(),
            parent_task_id: None,
            depends_on: vec![],
            assigned_tab_id: None,
            lane: Default::default(),
            taken_by_role: None,
            peer: None,
            group: None,
            canvas: Default::default(),
            milestones: vec![],
            execution_attempts: vec![],
            interruption: Default::default(),
            pause_reason: None,
            personality: None,
            personality_provenance: None,
            attachments: vec![],
        }
    }
}
