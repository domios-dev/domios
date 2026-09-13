//! Unified daemon-side reference resolution: full id | handle or task key | unique prefix.
//!
//! Every entity ref that reaches the daemon (plan / task / section / spec) flows through
//! one resolver. Ambiguity and unknown refs fail loudly with candidate lists; a ref is never
//! guessed. Old long uuid ids resolve exactly like freshly minted short ids.

use super::PlanStore;
use crate::plan::{task_key, Section, Spec, Task};
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    Plan,
    Task,
    Section,
    Spec,
    SpecSection,
    Loop,
    Element,
    Milestone,
}

impl RefKind {
    fn label(self) -> &'static str {
        match self {
            RefKind::Plan => "plan",
            RefKind::Task => "task",
            RefKind::Section => "section",
            RefKind::Spec => "spec",
            RefKind::SpecSection => "spec section",
            RefKind::Loop => "loop",
            RefKind::Element => "element",
            RefKind::Milestone => "milestone",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RefCandidate {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan_id: Option<String>,
}

#[derive(Debug)]
pub enum ResolveError {
    NotFound {
        kind: RefKind,
        reference: String,
    },
    Ambiguous {
        kind: RefKind,
        reference: String,
        candidates: Vec<RefCandidate>,
    },
}

impl ResolveError {
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            ResolveError::NotFound { kind, reference } => serde_json::json!({
                "type": "ref_not_found",
                "kind": kind.label(),
                "reference": reference,
            }),
            ResolveError::Ambiguous {
                kind,
                reference,
                candidates,
            } => serde_json::json!({
                "type": "ref_ambiguous",
                "kind": kind.label(),
                "reference": reference,
                "candidates": candidates,
            }),
        }
    }
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::NotFound { kind, reference } => {
                write!(f, "no {} matches '{reference}'", kind.label())
            }
            ResolveError::Ambiguous {
                kind,
                reference,
                candidates,
            } => {
                let listed = candidates
                    .iter()
                    .map(|c| match c.key.as_ref().or(c.handle.as_ref()) {
                        Some(name) => format!("{} ({}, {})", name, c.id, c.label),
                        None => format!("{} ({})", c.id, c.label),
                    })
                    .collect::<Vec<_>>()
                    .join("; ");
                write!(
                    f,
                    "{} ref '{reference}' is ambiguous, matches: {listed}",
                    kind.label()
                )
            }
        }
    }
}

#[derive(Clone)]
struct MatchItem {
    id: String,
    handle: Option<String>,
    key: Option<String>,
    label: String,
    plan_id: Option<String>,
}

impl MatchItem {
    fn into_candidate(self) -> RefCandidate {
        RefCandidate {
            id: self.id,
            handle: self.handle,
            key: self.key,
            label: self.label,
            plan_id: self.plan_id,
        }
    }

    fn task(task: &Task, plan_id: &str) -> Self {
        MatchItem {
            id: task.id.clone(),
            handle: task.handle.clone(),
            key: Some(task.display_key()),
            label: task.name.clone(),
            plan_id: Some(plan_id.to_string()),
        }
    }
}

/// Deterministic precedence: exact id > exact handle or task key (one tier, must be
/// unique) > unique prefix (on id or handle).
fn pick(items: Vec<MatchItem>, reference: &str, kind: RefKind) -> Result<MatchItem, ResolveError> {
    if let Some(hit) = items.iter().find(|i| i.id == reference) {
        return Ok(hit.clone());
    }
    let handle_hits: Vec<&MatchItem> = items
        .iter()
        .filter(|i| {
            i.handle.as_deref() == Some(reference)
                || i.key
                    .as_deref()
                    .is_some_and(|key| task_key::key_matches(key, reference))
        })
        .collect();
    match handle_hits.len() {
        1 => return Ok(handle_hits[0].clone()),
        n if n > 1 => {
            return Err(ambiguous(handle_hits, reference, kind));
        }
        _ => {}
    }
    if !reference.is_empty() {
        let prefix_hits: Vec<&MatchItem> = items
            .iter()
            .filter(|i| {
                i.id.starts_with(reference)
                    || i.handle
                        .as_deref()
                        .is_some_and(|h| h.starts_with(reference))
            })
            .collect();
        match prefix_hits.len() {
            1 => return Ok(prefix_hits[0].clone()),
            n if n > 1 => return Err(ambiguous(prefix_hits, reference, kind)),
            _ => {}
        }
    }
    Err(ResolveError::NotFound {
        kind,
        reference: reference.to_string(),
    })
}

fn ambiguous(hits: Vec<&MatchItem>, reference: &str, kind: RefKind) -> ResolveError {
    ResolveError::Ambiguous {
        kind,
        reference: reference.to_string(),
        candidates: hits
            .into_iter()
            .cloned()
            .map(MatchItem::into_candidate)
            .collect(),
    }
}

fn flatten<'a>(sections: &'a [Section], out: &mut Vec<&'a Section>) {
    for s in sections {
        out.push(s);
        flatten(&s.subsections, out);
    }
}

fn spec_section_items(spec: &Spec) -> Vec<MatchItem> {
    let mut flat = Vec::new();
    flatten(&spec.sections, &mut flat);
    flat.into_iter()
        .map(|s| MatchItem {
            id: s.id.clone(),
            key: None,
            handle: s.handle.clone(),
            label: s.title.clone(),
            plan_id: None,
        })
        .collect()
}

#[derive(Debug)]
pub struct ResolvedTask {
    pub plan_id: String,
    pub task_id: String,
}

#[derive(Debug)]
pub struct ResolvedSection {
    pub plan_id: String,
    pub section_id: String,
}

#[derive(Debug)]
pub struct ResolvedMilestone {
    pub plan_id: String,
    pub task_id: String,
    pub milestone_id: String,
}

impl PlanStore {
    /// Any id currently in the store (plan / spec / task / section / milestone). Mint-time check.
    pub(super) fn id_taken(&self, id: &str) -> bool {
        if self.specs.read().contains_key(id) {
            return true;
        }
        let plans = self.plans.read();
        if plans.contains_key(id) {
            return true;
        }
        plans.values().any(|arc| {
            let p = arc.lock();
            p.tasks.iter().any(|t| t.id == id)
                || p.tasks
                    .iter()
                    .any(|t| t.milestones.iter().any(|m| m.id == id))
                || section_tree_contains(&p.sections, id)
        })
    }

    /// Collision-checked short id. Must NOT be called while holding a plan mutex.
    pub(super) fn mint_id(&self, prefix: &str) -> String {
        crate::plan::ids::mint_id(prefix, |c| self.id_taken(c))
    }

    fn plan_match_items(&self) -> Vec<MatchItem> {
        self.plans
            .read()
            .values()
            .map(|arc| {
                let p = arc.lock();
                MatchItem {
                    id: p.id.clone(),
                    key: None,
                    handle: None,
                    label: p.title.clone(),
                    plan_id: None,
                }
            })
            .collect()
    }

    pub fn resolve_plan_id(&self, reference: &str) -> Result<String, ResolveError> {
        if self.plans.read().contains_key(reference) {
            return Ok(reference.to_string());
        }
        pick(self.plan_match_items(), reference, RefKind::Plan).map(|m| m.id)
    }

    pub fn resolve_spec_id(&self, reference: &str) -> Result<String, ResolveError> {
        if self.specs.read().contains_key(reference) {
            return Ok(reference.to_string());
        }
        let items = self
            .specs
            .read()
            .values()
            .map(|arc| {
                let s = arc.lock();
                MatchItem {
                    id: s.id.clone(),
                    key: None,
                    handle: None,
                    label: s.title.clone(),
                    plan_id: None,
                }
            })
            .collect();
        pick(items, reference, RefKind::Spec).map(|m| m.id)
    }

    /// Resolve a task by ref, with the owning plan resolved from the ref when `plan_ref` is absent.
    pub fn resolve_task_ref(
        &self,
        plan_ref: Option<&str>,
        task_ref: &str,
    ) -> Result<ResolvedTask, ResolveError> {
        let items = match plan_ref {
            Some(pr) => {
                let plan_id = self.resolve_plan_id(pr)?;
                self.task_items_for_plan(&plan_id)
            }
            None => self.task_items_all(),
        };
        pick(items, task_ref, RefKind::Task).map(|m| ResolvedTask {
            plan_id: m.plan_id.unwrap_or_default(),
            task_id: m.id,
        })
    }

    pub fn resolve_section_ref(
        &self,
        plan_ref: Option<&str>,
        section_ref: &str,
    ) -> Result<ResolvedSection, ResolveError> {
        let items = match plan_ref {
            Some(pr) => {
                let plan_id = self.resolve_plan_id(pr)?;
                self.section_items_for_plan(&plan_id)
            }
            None => self.section_items_all(),
        };
        pick(items, section_ref, RefKind::Section).map(|m| ResolvedSection {
            plan_id: m.plan_id.unwrap_or_default(),
            section_id: m.id,
        })
    }

    pub fn resolve_spec_section_id(
        &self,
        spec_ref: &str,
        section_ref: &str,
    ) -> Result<(String, String), ResolveError> {
        let spec_id = self.resolve_spec_id(spec_ref)?;
        let spec = self
            .specs
            .read()
            .get(&spec_id)
            .map(|arc| arc.lock().clone());
        let items = spec.map(|s| spec_section_items(&s)).unwrap_or_default();
        pick(items, section_ref, RefKind::SpecSection).map(|m| (spec_id, m.id))
    }

    pub fn resolve_loop_id(&self, reference: &str) -> Result<String, ResolveError> {
        let mut items = Vec::new();
        for arc in self.plans.read().values() {
            let p = arc.lock();
            for l in &p.loops {
                items.push(MatchItem {
                    id: l.id.clone(),
                    key: None,
                    handle: None,
                    label: l.target.clone(),
                    plan_id: Some(p.id.clone()),
                });
            }
        }
        pick(items, reference, RefKind::Loop).map(|m| m.id)
    }

    pub fn resolve_element_ref(
        &self,
        plan_ref: &str,
        element_ref: &str,
    ) -> Result<(String, String), ResolveError> {
        let plan_id = self.resolve_plan_id(plan_ref)?;
        let items = self
            .plans
            .read()
            .get(&plan_id)
            .map(|arc| {
                let p = arc.lock();
                p.elements
                    .iter()
                    .map(|e| MatchItem {
                        id: e.id.clone(),
                        key: None,
                        handle: None,
                        label: e.kind.clone(),
                        plan_id: Some(p.id.clone()),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        pick(items, element_ref, RefKind::Element).map(|m| (m.plan_id.unwrap_or(plan_id), m.id))
    }

    pub fn resolve_milestone_ref(
        &self,
        plan_ref: Option<&str>,
        task_ref: &str,
        milestone_ref: &str,
    ) -> Result<ResolvedMilestone, ResolveError> {
        let resolved_task = self.resolve_task_ref(plan_ref, task_ref)?;
        let plan_id = resolved_task.plan_id;
        let task_id = resolved_task.task_id;
        let items = self
            .plans
            .read()
            .get(&plan_id)
            .and_then(|arc| {
                let p = arc.lock();
                p.tasks.iter().find(|t| t.id == task_id).map(|task| {
                    task.milestones
                        .iter()
                        .map(|m| MatchItem {
                            id: m.id.clone(),
                            key: None,
                            handle: None,
                            label: m.kind.clone(),
                            plan_id: Some(plan_id.clone()),
                        })
                        .collect::<Vec<_>>()
                })
            })
            .unwrap_or_default();
        pick(items, milestone_ref, RefKind::Milestone).map(|m| ResolvedMilestone {
            plan_id,
            task_id,
            milestone_id: m.id,
        })
    }

    fn task_items_for_plan(&self, plan_id: &str) -> Vec<MatchItem> {
        self.plans
            .read()
            .get(plan_id)
            .map(|arc| {
                let p = arc.lock();
                p.tasks.iter().map(|t| MatchItem::task(t, &p.id)).collect()
            })
            .unwrap_or_default()
    }

    fn task_items_all(&self) -> Vec<MatchItem> {
        let mut out = Vec::new();
        for arc in self.plans.read().values() {
            let p = arc.lock();
            out.extend(p.tasks.iter().map(|t| MatchItem::task(t, &p.id)));
        }
        out
    }

    fn section_items_for_plan(&self, plan_id: &str) -> Vec<MatchItem> {
        self.plans
            .read()
            .get(plan_id)
            .map(|arc| {
                let p = arc.lock();
                let mut flat = Vec::new();
                flatten(&p.sections, &mut flat);
                flat.into_iter()
                    .map(|s| MatchItem {
                        id: s.id.clone(),
                        key: None,
                        handle: s.handle.clone(),
                        label: s.title.clone(),
                        plan_id: Some(p.id.clone()),
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn section_items_all(&self) -> Vec<MatchItem> {
        let mut out = Vec::new();
        for arc in self.plans.read().values() {
            let p = arc.lock();
            let mut flat = Vec::new();
            flatten(&p.sections, &mut flat);
            for s in flat {
                out.push(MatchItem {
                    id: s.id.clone(),
                    key: None,
                    handle: s.handle.clone(),
                    label: s.title.clone(),
                    plan_id: Some(p.id.clone()),
                });
            }
        }
        out
    }
}

fn section_tree_contains(sections: &[Section], id: &str) -> bool {
    sections
        .iter()
        .any(|s| s.id == id || section_tree_contains(&s.subsections, id))
}

#[cfg(test)]
mod tests {
    use super::{RefKind, ResolveError};
    use crate::plan::{OrchestratorRef, Plan, Task, TaskPriority, TaskStatus};
    use crate::service_plan::{NewTask, PlanStore};

    fn store_with(dir: &std::path::Path) -> PlanStore {
        PlanStore::new_for_test(dir)
    }

    fn mk_plan(store: &PlanStore, dir: &std::path::Path, title: &str) -> String {
        store
            .create_plan(
                title.into(),
                dir.display().to_string(),
                OrchestratorRef::from_tab("p"),
                vec![],
                1000,
            )
            .unwrap()
            .id
    }

    fn add_task(store: &PlanStore, plan_id: &str, name: &str, handle: &str) -> String {
        store
            .add_task(
                plan_id,
                NewTask {
                    name: name.into(),
                    kind: "task".into(),
                    handle: Some(handle.into()),
                    competition: None,
                    ..Default::default()
                },
                2000,
            )
            .unwrap()
    }

    fn legacy_task(id: &str, name: &str, handle: &str) -> Task {
        Task {
            id: id.into(),
            name: name.into(),
            kind: "task".into(),
            model: None,
            model_rationale: None,
            acceptance_criteria: vec![],
            external_refs: vec![],
            test_plan: None,
            qa_plan: None,
            correctness: None,
            status: TaskStatus::Pending,
            review_stage: Default::default(),
            qa_state: None,
            review_round: 0,
            review_oracle: None,
            review_evidence: vec![],
            key: None,
            review_override: None,
            review_audit: vec![],
            status_label: None,
            priority: TaskPriority::default(),
            created_at: 0,
            updated_at: 0,
            status_changed_at: 0,
            handle: Some(handle.into()),
            tags: vec![],
            section_ids: vec![],
            body: String::new(),
            role_guidance: None,
            intent: None,
            execution_intent: Default::default(),
            parent_task_id: None,
            depends_on: vec![],
            assigned_tab_id: None,
            lane: Default::default(),
            taken_by_role: None,
            executor_role: None,
            competition: None,
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

    #[test]
    fn minted_ids_are_short_and_prefixed() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with(dir.path());
        let plan_id = mk_plan(&store, dir.path(), "P");
        assert!(
            plan_id.starts_with("pl_"),
            "plan id keeps prefix: {plan_id}"
        );
        let slug = plan_id.split_once('_').unwrap().1;
        assert!(
            (6..=8).contains(&slug.len()),
            "plan slug must be 6-8 chars, got {} ({plan_id})",
            slug.len()
        );
        let task_id = add_task(&store, &plan_id, "T", "alpha");
        let tslug = task_id.split_once('_').unwrap().1;
        assert!(task_id.starts_with("task_"));
        assert!((6..=8).contains(&tslug.len()), "task slug 6-8: {task_id}");
    }

    #[test]
    fn resolves_plan_by_full_id_and_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with(dir.path());
        let plan_id = mk_plan(&store, dir.path(), "P");
        assert_eq!(store.resolve_plan_id(&plan_id).unwrap(), plan_id);
        let prefix = &plan_id[..plan_id.len() - 2];
        assert_eq!(store.resolve_plan_id(prefix).unwrap(), plan_id);
    }

    #[test]
    fn resolves_task_parent_free_by_handle_and_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with(dir.path());
        let plan_a = mk_plan(&store, dir.path(), "A");
        let plan_b = mk_plan(&store, dir.path(), "B");
        let task_a = add_task(&store, &plan_a, "Alpha", "alpha");
        let _task_b = add_task(&store, &plan_b, "Beta", "beta");

        // Handle, no plan ref -> resolves across plans.
        let r = store.resolve_task_ref(None, "alpha").unwrap();
        assert_eq!(r.plan_id, plan_a);
        assert_eq!(r.task_id, task_a);

        // 4-char id prefix, no plan ref.
        let prefix = &task_a[..9]; // "task_" + 4 chars
        let r = store.resolve_task_ref(None, prefix).unwrap();
        assert_eq!(r.task_id, task_a);

        // Full id still resolves.
        let r = store.resolve_task_ref(None, &task_a).unwrap();
        assert_eq!(r.task_id, task_a);
    }

    #[test]
    fn ambiguous_handle_prefix_lists_candidates() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with(dir.path());
        let plan = mk_plan(&store, dir.path(), "P");
        add_task(&store, &plan, "Login", "auth-login");
        add_task(&store, &plan, "Logout", "auth-logout");

        let err = store.resolve_task_ref(Some(&plan), "auth").unwrap_err();
        match err {
            ResolveError::Ambiguous {
                kind, candidates, ..
            } => {
                assert_eq!(kind, RefKind::Task);
                assert_eq!(candidates.len(), 2);
            }
            other => panic!("expected ambiguity, got {other:?}"),
        }
    }

    #[test]
    fn exact_handle_wins_over_prefix_of_another() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with(dir.path());
        let plan = mk_plan(&store, dir.path(), "P");
        let short = add_task(&store, &plan, "Auth", "auth");
        add_task(&store, &plan, "Auth login", "auth-login");
        // "auth" is an exact handle AND a prefix of "auth-login"; exact wins, no ambiguity.
        let r = store.resolve_task_ref(Some(&plan), "auth").unwrap();
        assert_eq!(r.task_id, short);
    }

    #[test]
    fn unknown_ref_never_guesses() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with(dir.path());
        let plan = mk_plan(&store, dir.path(), "P");
        add_task(&store, &plan, "Alpha", "alpha");
        match store.resolve_task_ref(None, "zzz-nope").unwrap_err() {
            ResolveError::NotFound { kind, .. } => assert_eq!(kind, RefKind::Task),
            other => panic!("expected not found, got {other:?}"),
        }
    }

    #[test]
    fn legacy_long_ids_still_resolve() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with(dir.path());
        let long_plan = "pl_deadbeefdeadbeefdeadbeefdeadbeef";
        let long_task = "task_cafebabecafebabecafebabecafebabe";
        let mut plan = Plan::new(
            "Legacy".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("p"),
        );
        plan.id = long_plan.into();
        plan.tasks
            .push(legacy_task(long_task, "Old", "legacy-task"));
        store.save(&mut plan, 1000).unwrap();

        assert_eq!(store.resolve_plan_id(long_plan).unwrap(), long_plan);
        let r = store.resolve_task_ref(None, long_task).unwrap();
        assert_eq!(r.plan_id, long_plan);
        assert_eq!(r.task_id, long_task);
        // Legacy id still resolvable by handle and by prefix.
        assert_eq!(
            store.resolve_task_ref(None, "legacy-task").unwrap().task_id,
            long_task
        );
        assert_eq!(
            store
                .resolve_task_ref(None, &long_task[..12])
                .unwrap()
                .task_id,
            long_task
        );
    }

    #[test]
    fn resolves_section_parent_free() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with(dir.path());
        let plan = mk_plan(&store, dir.path(), "P");
        let sid = store
            .add_section(&plan, "Design", "", None, &[], None, Some("design"), 2000)
            .unwrap();
        let r = store.resolve_section_ref(None, "design").unwrap();
        assert_eq!(r.plan_id, plan);
        assert_eq!(r.section_id, sid);
        let r = store.resolve_section_ref(None, &sid).unwrap();
        assert_eq!(r.section_id, sid);
    }

    #[test]
    fn id_taken_reports_all_entity_kinds() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_with(dir.path());
        let plan = mk_plan(&store, dir.path(), "P");
        let task = add_task(&store, &plan, "T", "alpha");
        assert!(store.id_taken(&plan));
        assert!(store.id_taken(&task));
        assert!(!store.id_taken("task_notreal"));
    }
}
