use super::types::{Plan, Section, TaskStatus};
use std::collections::HashMap;

#[derive(Debug, Default, PartialEq)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::types::{Plan, Section, Task, TaskStatus};

    fn section(id: &str) -> Section {
        Section {
            id: id.into(),
            title: id.into(),
            body: String::new(),
            subsections: vec![],
            canvas: Default::default(),
        }
    }

    fn task(sections: &[&str], status: TaskStatus) -> Task {
        Task {
            id: "t".into(),
            name: "n".into(),
            kind: "implement".into(),
            model: None,
            model_rationale: None,
            status,
            status_label: None,
            section_ids: sections.iter().map(|s| s.to_string()).collect(),
            body: String::new(),
            intent: None,
            assigned_pane_id: None,
            peer: None,
            canvas: Default::default(),
            milestones: vec![],
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
}
