use super::types::{Plan, Section, Spec, SpecLink, TaskStatus};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct SectionCoverage {
    pub total: usize,
    pub done: usize,
    pub in_progress: usize,
}

#[derive(Debug, Default, PartialEq)]
pub struct Coverage {
    pub per_section: HashMap<String, SectionCoverage>,
    pub plan_covered_fraction: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CoverageSectionInput {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CoverageTaskInput {
    pub name: String,
    pub status: TaskStatus,
    pub section_ids: Vec<String>,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub struct PlanCoverageInput {
    pub sections: Vec<CoverageSectionInput>,
    pub tasks: Vec<CoverageTaskInput>,
    pub spec_links: Vec<SpecLink>,
}

#[derive(Debug, Deserialize)]
pub struct SpecCoverageInput {
    pub id: String,
    pub sections: Vec<CoverageSectionInput>,
}

fn section_inputs(sections: &[Section]) -> Vec<CoverageSectionInput> {
    let mut inputs = Vec::new();
    for section in sections {
        inputs.push(CoverageSectionInput {
            id: section.id.clone(),
            title: section.title.clone(),
        });
        inputs.extend(section_inputs(&section.subsections));
    }
    inputs
}

impl From<&Plan> for PlanCoverageInput {
    fn from(plan: &Plan) -> Self {
        Self {
            sections: section_inputs(&plan.sections),
            tasks: plan
                .tasks
                .iter()
                .map(|task| CoverageTaskInput {
                    name: task.name.clone(),
                    status: task.status.clone(),
                    section_ids: task.section_ids.clone(),
                })
                .collect(),
            spec_links: plan.spec_links.clone(),
        }
    }
}

impl From<&Spec> for SpecCoverageInput {
    fn from(spec: &Spec) -> Self {
        Self {
            id: spec.id.clone(),
            sections: section_inputs(&spec.sections),
        }
    }
}

pub fn compute_coverage(input: impl Into<PlanCoverageInput>) -> Coverage {
    coverage_for(&input.into())
}

fn coverage_for(plan: &PlanCoverageInput) -> Coverage {
    let ids = plan
        .sections
        .iter()
        .map(|section| section.id.as_str())
        .collect::<Vec<_>>();
    let mut per: HashMap<String, SectionCoverage> = HashMap::new();
    for id in &ids {
        per.entry((*id).into()).or_default();
    }
    for t in &plan.tasks {
        for sid in &t.section_ids {
            let e = per.entry(sid.clone()).or_default();
            e.total += 1;
            match t.status {
                TaskStatus::Done => e.done += 1,
                TaskStatus::InProgress => e.in_progress += 1,
                _ => {}
            }
        }
    }
    let covered = ids
        .iter()
        .filter(|id| per.get(**id).map_or(false, |c| c.total > 0))
        .count();
    let plan_covered_fraction = if ids.is_empty() {
        0.0
    } else {
        covered as f64 / ids.len() as f64
    };
    Coverage {
        per_section: per,
        plan_covered_fraction,
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SectionCoverageReport {
    pub section_id: String,
    pub title: String,
    pub total: usize,
    pub done: usize,
    pub in_progress: usize,
    pub status: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CoverageReport {
    pub plan_covered_fraction: f64,
    pub sections: Vec<SectionCoverageReport>,
    pub gaps: Vec<String>,
    pub unlinked_count: usize,
    pub unlinked_tasks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SpecSectionCoverageReport {
    pub section_id: String,
    pub title: String,
    pub linked_plan_sections: Vec<String>,
    pub total: usize,
    pub done: usize,
    pub in_progress: usize,
    pub status: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct SpecCoverageReport {
    pub spec_covered_fraction: f64,
    pub sections: Vec<SpecSectionCoverageReport>,
    pub unplanned: Vec<String>,
}

fn rollup(c: &SectionCoverage) -> &'static str {
    if c.total == 0 {
        "none"
    } else if c.done == c.total {
        "done"
    } else if c.in_progress > 0 || c.done > 0 {
        "in_progress"
    } else {
        "pending"
    }
}

/// Coverage keyed by section TITLE with a per-section status rollup and a list
/// of uncovered (0-task) section titles. Computed on read, never stored.
pub fn compute_coverage_report(input: impl Into<PlanCoverageInput>) -> CoverageReport {
    let plan = input.into();
    let cov = coverage_for(&plan);
    let ordered = plan
        .sections
        .iter()
        .map(|section| (section.id.as_str(), section.title.as_str()));

    let mut sections = Vec::with_capacity(plan.sections.len());
    let mut gaps = vec![];
    for (id, title) in ordered {
        let c = cov.per_section.get(id).cloned().unwrap_or_default();
        let status = rollup(&c);
        if status == "none" {
            gaps.push(title.to_string());
        }
        sections.push(SectionCoverageReport {
            section_id: id.to_string(),
            title: title.to_string(),
            total: c.total,
            done: c.done,
            in_progress: c.in_progress,
            status,
        });
    }
    let unlinked_tasks = plan
        .tasks
        .iter()
        .filter(|task| task.section_ids.is_empty())
        .map(|task| task.name.clone())
        .collect::<Vec<_>>();
    CoverageReport {
        plan_covered_fraction: cov.plan_covered_fraction,
        sections,
        gaps,
        unlinked_count: unlinked_tasks.len(),
        unlinked_tasks,
    }
}

pub fn compute_spec_coverage_report(
    spec: impl Into<SpecCoverageInput>,
    plans: &[PlanCoverageInput],
) -> SpecCoverageReport {
    let spec = spec.into();
    let ordered = spec
        .sections
        .iter()
        .map(|section| (section.id.as_str(), section.title.as_str()));

    let mut links_by_spec: HashMap<String, Vec<(&PlanCoverageInput, &SpecLink)>> = HashMap::new();
    for plan in plans {
        for link in plan
            .spec_links
            .iter()
            .filter(|link| link.spec_id == spec.id)
        {
            links_by_spec
                .entry(link.spec_section_id.clone())
                .or_default()
                .push((plan, link));
        }
    }

    let mut sections = Vec::with_capacity(spec.sections.len());
    let mut unplanned = vec![];
    for (id, title) in ordered {
        let links = links_by_spec.get(id).cloned().unwrap_or_default();
        let mut coverage = SectionCoverage::default();
        let mut linked_plan_sections = vec![];
        for (plan, link) in links {
            linked_plan_sections.push(link.plan_section_id.clone());
            for task in &plan.tasks {
                if task
                    .section_ids
                    .iter()
                    .any(|sid| sid == &link.plan_section_id)
                {
                    coverage.total += 1;
                    match task.status {
                        TaskStatus::Done => coverage.done += 1,
                        TaskStatus::InProgress => coverage.in_progress += 1,
                        _ => {}
                    }
                }
            }
        }
        linked_plan_sections.sort();
        linked_plan_sections.dedup();
        let status = if linked_plan_sections.is_empty() {
            unplanned.push(title.to_string());
            "unplanned"
        } else {
            rollup(&coverage)
        };
        sections.push(SpecSectionCoverageReport {
            section_id: id.to_string(),
            title: title.to_string(),
            linked_plan_sections,
            total: coverage.total,
            done: coverage.done,
            in_progress: coverage.in_progress,
            status,
        });
    }
    let covered = sections
        .iter()
        .filter(|section| section.status == "done")
        .count();
    let spec_covered_fraction = if sections.is_empty() {
        0.0
    } else {
        covered as f64 / sections.len() as f64
    };
    SpecCoverageReport {
        spec_covered_fraction,
        sections,
        unplanned,
    }
}

#[cfg(test)]
mod tests;
