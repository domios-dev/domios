//! Unified daemon-side reference resolution: full id | handle or task key | unique prefix.
//!
//! Every entity ref that reaches the daemon (plan / task / section / spec) flows through
//! one resolver. Ambiguity and unknown refs fail loudly with candidate lists; a ref is never
//! guessed. Old long uuid ids resolve exactly like freshly minted short ids.

use super::PlanStore;
use crate::plan::{task_key, Section, Spec};
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
}

/// Deterministic precedence: exact id > exact handle or task key (one tier, must be
/// unique) > unique prefix (on id or handle).
fn pick(items: Vec<MatchItem>, reference: &str, kind: RefKind) -> Result<MatchItem, ResolveError> {
    if let Some(hit) = items.iter().find(|i| i.id == reference) {
        return Ok(hit.clone());
    }
    let handle_hits: Vec<&MatchItem> = items
        .iter()
        .filter(|i| match i.key.as_deref() {
            Some(key) => task_key::name_matches(i.handle.as_deref(), key, reference),
            None => i.handle.as_deref() == Some(reference),
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
        if self.specs.read().contains_key(id) || self.index.read().contains_key(id) {
            return true;
        }
        self.database.id_in_use(id).unwrap_or_else(|error| {
            log::warn!("[plan.store] id collision query failed: {error}");
            false
        })
    }

    /// Collision-checked short id. Must NOT be called while holding a plan mutex.
    pub(super) fn mint_id(&self, prefix: &str) -> String {
        crate::plan::ids::mint_id(prefix, |c| self.id_taken(c))
    }

    fn plan_match_items(&self) -> Vec<MatchItem> {
        self.index
            .read()
            .values()
            .map(|row| MatchItem {
                id: row.id.clone(),
                key: None,
                handle: None,
                label: row.title.clone(),
                plan_id: None,
            })
            .collect()
    }

    pub fn resolve_plan_id(&self, reference: &str) -> Result<String, ResolveError> {
        if self.index.read().contains_key(reference) {
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
        let items = self
            .loop_rows()
            .into_iter()
            .map(|(plan_id, id, target)| MatchItem {
                id,
                key: None,
                handle: None,
                label: target,
                plan_id: Some(plan_id),
            })
            .collect();
        pick(items, reference, RefKind::Loop).map(|m| m.id)
    }

    pub fn resolve_element_ref(
        &self,
        plan_ref: &str,
        element_ref: &str,
    ) -> Result<(String, String), ResolveError> {
        let plan_id = self.resolve_plan_id(plan_ref)?;
        let items = self
            .database
            .element_references(&plan_id)
            .unwrap_or_default()
            .into_iter()
            .map(|(id, label)| MatchItem {
                id,
                label,
                key: None,
                handle: None,
                plan_id: Some(plan_id.clone()),
            })
            .collect();
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
            .database
            .milestone_references(&plan_id, &task_id)
            .unwrap_or_default()
            .into_iter()
            .map(|(id, label)| MatchItem {
                id,
                label,
                key: None,
                handle: None,
                plan_id: Some(plan_id.clone()),
            })
            .collect();
        pick(items, milestone_ref, RefKind::Milestone).map(|m| ResolvedMilestone {
            plan_id,
            task_id,
            milestone_id: m.id,
        })
    }

    fn task_items_for_plan(&self, plan_id: &str) -> Vec<MatchItem> {
        self.database
            .scoped_task_references(plan_id)
            .unwrap_or_default()
            .into_iter()
            .map(|row| MatchItem {
                key: Some(
                    row.key
                        .unwrap_or_else(|| task_key::fallback_key(&row.task_id)),
                ),
                id: row.task_id,
                handle: row.handle,
                label: row.name,
                plan_id: Some(row.plan_id),
            })
            .collect()
    }

    /// Read from the task rows; no plan is loaded.
    fn task_items_all(&self) -> Vec<MatchItem> {
        self.task_ref_rows()
            .into_iter()
            .map(|row| MatchItem {
                key: Some(
                    row.key
                        .unwrap_or_else(|| crate::plan::task_key::fallback_key(&row.task_id)),
                ),
                id: row.task_id,
                handle: row.handle,
                label: row.name,
                plan_id: Some(row.plan_id),
            })
            .collect()
    }

    fn section_items_for_plan(&self, plan_id: &str) -> Vec<MatchItem> {
        self.database
            .scoped_section_references(plan_id)
            .unwrap_or_default()
            .into_iter()
            .map(|row| MatchItem {
                id: row.section_id,
                key: None,
                handle: row.handle,
                label: row.title,
                plan_id: Some(row.plan_id),
            })
            .collect()
    }

    /// Read from the section rows, nested subsections included; no plan is loaded.
    fn section_items_all(&self) -> Vec<MatchItem> {
        self.section_ref_rows()
            .into_iter()
            .map(|row| MatchItem {
                id: row.section_id,
                key: None,
                handle: row.handle,
                label: row.title,
                plan_id: Some(row.plan_id),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
