use super::types::{Plan, Section, Spec, SpecLink, TaskStatus};
use serde::Serialize;
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

pub fn compute_coverage(plan: &Plan) -> Coverage {
    fn walk<'a>(sections: &'a [Section], ids: &mut Vec<&'a str>) {
        for sec in sections {
            ids.push(&sec.id);
            walk(&sec.subsections, ids);
        }
    }
    let mut ids = vec![];
    walk(&plan.sections, &mut ids);

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
pub fn compute_coverage_report(plan: &Plan) -> CoverageReport {
    fn walk<'a>(sections: &'a [Section], out: &mut Vec<(&'a str, &'a str)>) {
        for sec in sections {
            out.push((&sec.id, &sec.title));
            walk(&sec.subsections, out);
        }
    }
    let cov = compute_coverage(plan);
    let mut ordered = vec![];
    walk(&plan.sections, &mut ordered);

    let mut sections = Vec::with_capacity(ordered.len());
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

pub fn compute_spec_coverage_report(spec: &Spec, plans: &[Plan]) -> SpecCoverageReport {
    fn walk<'a>(sections: &'a [Section], out: &mut Vec<(&'a str, &'a str)>) {
        for sec in sections {
            out.push((&sec.id, &sec.title));
            walk(&sec.subsections, out);
        }
    }
    let mut ordered = vec![];
    walk(&spec.sections, &mut ordered);

    let mut links_by_spec: HashMap<String, Vec<(&Plan, &SpecLink)>> = HashMap::new();
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

    let mut sections = Vec::with_capacity(ordered.len());
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
mod tests {
    use super::*;
    use crate::plan::types::{Plan, Section, Task, TaskStatus};

    fn section(id: &str) -> Section {
        Section {
            id: id.into(),
            title: id.into(),
            handle: None,
            tags: vec![],
            body: String::new(),
            subsections: vec![],
            canvas: Default::default(),
            attachments: vec![],
        }
    }

    fn task(sections: &[&str], status: TaskStatus) -> Task {
        Task {
            id: "t".into(),
            name: "n".into(),
            kind: "implement".into(),
            model: None,
            model_rationale: None,
            acceptance_criteria: vec![],
            test_plan: None,
            qa_plan: None,
            correctness: None,
            status,
            review_stage: Default::default(),
            qa_state: None,
            review_round: 0,
            review_oracle: None,
            review_evidence: vec![],
            review_override: None,
            review_audit: vec![],
            status_label: None,
            priority: Default::default(),
            created_at: 0,
            updated_at: 0,
            status_changed_at: 0,
            handle: None,
            tags: vec![],
            section_ids: sections.iter().map(|s| s.to_string()).collect(),
            body: String::new(),
            role_guidance: None,
            executor_role: None,
            competition: None,
            intent: None,
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
            pause_reason: None,
            personality: None,
            personality_provenance: None,
            attachments: vec![],
        }
    }

    fn plan_with(sections: &[&str], tasks: &[(&[&str], TaskStatus)]) -> Plan {
        let mut p = Plan::new("T".into(), "/repo".into(), Default::default());
        p.sections = sections.iter().map(|s| section(s)).collect();
        p.tasks = tasks.iter().map(|(s, st)| task(s, st.clone())).collect();
        p
    }

    #[test]
    fn coverage_counts_tasks_per_section() {
        let cov = compute_coverage(&plan_with(
            &["s1", "s2"],
            &[(&["s1"], TaskStatus::Done), (&["s1"], TaskStatus::Pending)],
        ));
        assert_eq!(cov.per_section["s1"].total, 2);
        assert_eq!(cov.per_section["s1"].done, 1);
        assert_eq!(cov.per_section["s2"].total, 0);
        assert!((cov.plan_covered_fraction - 0.5).abs() < 1e-9);
    }

    #[test]
    fn report_lists_gaps_and_rolls_up_status() {
        let report = compute_coverage_report(&plan_with(
            &["s1", "s2"],
            &[(&["s1"], TaskStatus::Done), (&["s1"], TaskStatus::Pending)],
        ));
        assert_eq!(report.gaps, vec!["s2".to_string()]);
        assert_eq!(report.unlinked_count, 0);
        assert!((report.plan_covered_fraction - 0.5).abs() < 1e-9);
        let s1 = report.sections.iter().find(|s| s.title == "s1").unwrap();
        assert_eq!(s1.total, 2);
        assert_eq!(s1.done, 1);
        assert_eq!(s1.status, "in_progress");
        let s2 = report.sections.iter().find(|s| s.title == "s2").unwrap();
        assert_eq!(s2.status, "none");
    }

    #[test]
    fn report_counts_unlinked_tasks() {
        let report = compute_coverage_report(&plan_with(
            &["s1"],
            &[(&[], TaskStatus::Pending), (&["s1"], TaskStatus::Done)],
        ));
        assert_eq!(report.unlinked_count, 1);
        assert_eq!(report.unlinked_tasks, vec!["n".to_string()]);
    }
}
