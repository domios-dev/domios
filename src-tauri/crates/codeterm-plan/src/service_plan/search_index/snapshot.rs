use super::*;

/// Search-only materialized units; cannot be consumed as authority or mutation state.
pub(in crate::service_plan) struct PlanSearchSnapshot {
    pub(super) id: String,
    pub(super) root: PathBuf,
    pub(super) units: Vec<Unit>,
}

impl PlanSearchSnapshot {
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
