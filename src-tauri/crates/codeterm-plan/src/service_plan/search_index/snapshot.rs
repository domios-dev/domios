use super::*;

/// Search-only materialized units; cannot be consumed as authority or mutation state.
pub(in crate::service_plan) struct PlanSearchSnapshot {
    pub(super) id: String,
    pub(super) root: PathBuf,
    pub(super) units: Vec<Unit>,
}

#[derive(serde::Deserialize)]
pub(in crate::service_plan) struct SearchHeaderRead {
    pub id: String,
    pub title: String,
    pub status: PlanStatus,
    pub project_path: String,
    pub canonical_project_path: String,
    pub updated_at: u64,
}

#[derive(serde::Deserialize)]
pub(in crate::service_plan) struct SearchSectionRead {
    pub id: String,
    pub title: String,
    pub body: String,
}

#[derive(serde::Deserialize)]
pub(in crate::service_plan) struct SearchMilestoneRead {
    pub id: String,
    pub kind: String,
    pub text: String,
    pub ts: u64,
}

#[derive(serde::Deserialize)]
pub(in crate::service_plan) struct SearchTaskRead {
    pub id: String,
    pub name: String,
    pub status: crate::plan::TaskStatus,
    pub updated_at: u64,
    pub body: String,
    pub acceptance_criteria: Vec<String>,
    pub external_refs: Vec<String>,
    #[serde(skip)]
    pub milestones: Vec<SearchMilestoneRead>,
}

pub(in crate::service_plan) struct PlanSearchInput {
    pub header: SearchHeaderRead,
    pub sections: Vec<(bool, SearchSectionRead)>,
    pub tasks: Vec<SearchTaskRead>,
}

impl PlanSearchSnapshot {
    pub(in crate::service_plan) fn from_input(input: PlanSearchInput) -> Self {
        use super::unit_inputs::{StrideUnitInput, TaskUnitInput, UnitParent};
        let header = input.header;
        let root = PathBuf::from(if header.canonical_project_path.trim().is_empty() {
            &header.project_path
        } else {
            &header.canonical_project_path
        });
        let scope = project_scope(&root);
        let status = plan_status_text(&header.status);
        let parent = UnitParent {
            id: &header.id,
            title: &header.title,
            scope: &scope,
            status: &status,
        };
        let titles = input
            .sections
            .iter()
            .filter(|(root, _)| *root)
            .map(|(_, section)| section.title.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let mut units = vec![parent.header(&titles, header.updated_at)];
        let total = count_sections(&input.sections, |_| &[]);
        for (index, (_, section)) in input.sections.into_iter().enumerate() {
            units.push(parent.section(
                &section.id,
                &section.title,
                &section.body,
                index + 1,
                total,
                header.updated_at,
            ));
        }
        for task in input.tasks {
            units.push(parent.task(TaskUnitInput {
                id: &task.id,
                name: &task.name,
                status: &task.status,
                updated_at: task.updated_at,
                body: &task.body,
                acceptance_criteria: &task.acceptance_criteria,
                external_refs: task.external_refs.iter().map(String::as_str).collect(),
            }));
            for stride in task.milestones {
                units.push(parent.stride(
                    &task.name,
                    task.status.key(),
                    StrideUnitInput {
                        id: &stride.id,
                        kind: &stride.kind,
                        text: &stride.text,
                        ts: stride.ts,
                    },
                ));
            }
        }
        Self {
            id: header.id,
            root,
            units,
        }
    }

    pub(in crate::service_plan) fn from_plan(plan: &Plan) -> Self {
        let root = plan_project_root(plan).to_path_buf();
        Self {
            id: plan.id.clone(),
            units: plan_units(plan, &project_scope(&root)),
            root,
        }
    }

    #[cfg(test)]
    pub(in crate::service_plan) fn assert_matches(&self, plan: &Plan) -> usize {
        let expected = plan_units(plan, &project_scope(plan_project_root(plan)));
        assert_eq!(
            self.units.iter().map(unit_signature).collect::<Vec<_>>(),
            expected.iter().map(unit_signature).collect::<Vec<_>>()
        );
        serde_json::to_vec(&self.units).unwrap().len()
    }

    #[cfg(test)]
    pub(in crate::service_plan) fn assert_section_ordinals_for_test(
        &self,
        sections: &[(&str, &str, usize)],
    ) {
        for &(id, title, ordinal) in sections {
            let unit = self
                .units
                .iter()
                .find(|unit| unit.kind == "plan_section" && unit.unit_id == id)
                .unwrap();
            assert!(unit
                .body
                .contains(&format!("section {ordinal}/{}: {title}\n", sections.len())));
        }
    }

    #[cfg(test)]
    pub(in crate::service_plan) fn assert_coherent_test_labels(&self) {
        let title = &self.units[0].title;
        let section = self
            .units
            .iter()
            .find(|unit| unit.unit_id == "sec_a")
            .unwrap();
        let task = self.units.iter().find(|unit| unit.kind == "task").unwrap();
        let milestone = self
            .units
            .iter()
            .find(|unit| unit.kind == "stride")
            .unwrap();
        assert_eq!(&section.unit_title, title);
        assert_eq!(&task.unit_title, title);
        assert!(milestone.body.ends_with(&format!(": {title}")));
    }
}
