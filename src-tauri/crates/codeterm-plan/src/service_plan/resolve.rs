//! Unified daemon-side reference resolution: full id | handle or task key | unique prefix.
//!
//! Every entity ref that reaches the daemon (plan / task / section / spec) flows through
//! one resolver. Ambiguity and unknown refs fail loudly with candidate lists; a ref is never
//! guessed. Old long uuid ids resolve exactly like freshly minted short ids.

use super::{PlanError, PlanStore};
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
    Read(PlanError),
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

impl From<PlanError> for ResolveError {
    fn from(error: PlanError) -> Self {
        Self::Read(error)
    }
}

impl ResolveError {
    pub fn to_json(&self) -> serde_json::Value {
        match self {
            ResolveError::Read(error) => error.to_json(),
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
            ResolveError::Read(error) => write!(f, "reference lookup failed: {error}"),
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
pub(super) fn match_rank(
    id: &str,
    handle: Option<&str>,
    key: Option<&str>,
    reference: &str,
) -> Option<u8> {
    if id == reference {
        return Some(0);
    }
    let named = match key {
        Some(key) => task_key::name_matches(handle, key, reference),
        None => handle == Some(reference),
    };
    if named {
        return Some(1);
    }
    if !reference.is_empty()
        && (id.starts_with(reference) || handle.is_some_and(|handle| handle.starts_with(reference)))
    {
        return Some(2);
    }
    None
}

fn pick(items: Vec<MatchItem>, reference: &str, kind: RefKind) -> Result<MatchItem, ResolveError> {
    for rank in 0..=2 {
        let hits: Vec<_> = items
            .iter()
            .filter(|item| {
                match_rank(
                    &item.id,
                    item.handle.as_deref(),
                    item.key.as_deref(),
                    reference,
                ) == Some(rank)
            })
            .collect();
        match hits.len() {
            0 => {}
            1 => return Ok(hits[0].clone()),
            _ if rank == 0 => return Ok(hits[0].clone()),
            _ => return Err(ambiguous(hits, reference, kind)),
        }
    }
    Err(ResolveError::NotFound {
        kind,
        reference: reference.into(),
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

pub(super) struct TaskReferenceRead {
    pub plan_id: String,
    pub task_id: String,
    pub candidate: Option<RefCandidate>,
}

impl PlanStore {
    /// Any id currently in the store (plan / spec / task / section / milestone). Mint-time check.
    pub(super) fn id_taken(&self, id: &str) -> Result<bool, PlanError> {
        if self.specs.read().contains_key(id) || self.index.read().contains_key(id) {
            return Ok(true);
        }
        self.database.id_in_use(id)
    }

    /// Collision-checked short id. Must NOT be called while holding a plan mutex.
    pub(super) fn mint_id(&self, prefix: &str) -> Result<String, PlanError> {
        crate::plan::ids::try_mint_id(prefix, |candidate| self.id_taken(candidate))
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
        let plan_id = plan_ref
            .map(|reference| self.resolve_plan_id(reference))
            .transpose()?;
        let mut matches = self
            .database
            .resolve_task_reference(plan_id.as_deref(), task_ref)?;
        if matches.len() == 1 {
            let found = matches.remove(0);
            return Ok(ResolvedTask {
                plan_id: found.plan_id,
                task_id: found.task_id,
            });
        }
        if matches.is_empty() {
            return Err(ResolveError::NotFound {
                kind: RefKind::Task,
                reference: task_ref.into(),
            });
        }
        let candidates = matches
            .into_iter()
            .map(|matched| {
                matched.candidate.ok_or_else(|| {
                    PlanError::Io("ambiguous task reference lacks diagnostic metadata".into())
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Err(ResolveError::Ambiguous {
            kind: RefKind::Task,
            reference: task_ref.into(),
            candidates,
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
                self.section_items_for_plan(&plan_id)?
            }
            None => self.section_items_all()?,
        };
        let matched = pick(items, section_ref, RefKind::Section)?;
        let plan_id = matched
            .plan_id
            .ok_or_else(|| PlanError::Io("section reference lacks an owning plan".into()))?;
        Ok(ResolvedSection {
            plan_id,
            section_id: matched.id,
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
            .loop_rows()?
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
            .element_references(&plan_id)?
            .into_iter()
            .map(|(id, label)| MatchItem {
                id,
                label,
                key: None,
                handle: None,
                plan_id: Some(plan_id.clone()),
            })
            .collect();
        pick(items, element_ref, RefKind::Element).map(|m| (plan_id, m.id))
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
            .milestone_references(&plan_id, &task_id)?
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

    fn section_items_for_plan(&self, plan_id: &str) -> Result<Vec<MatchItem>, PlanError> {
        Ok(self
            .database
            .scoped_section_references(plan_id)?
            .into_iter()
            .map(|row| MatchItem {
                id: row.section_id,
                key: None,
                handle: row.handle,
                label: row.title,
                plan_id: Some(row.plan_id),
            })
            .collect())
    }

    /// Read from the section rows, nested subsections included; no plan is loaded.
    fn section_items_all(&self) -> Result<Vec<MatchItem>, PlanError> {
        Ok(self
            .section_ref_rows()?
            .into_iter()
            .map(|row| MatchItem {
                id: row.section_id,
                key: None,
                handle: row.handle,
                label: row.title,
                plan_id: Some(row.plan_id),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests;
