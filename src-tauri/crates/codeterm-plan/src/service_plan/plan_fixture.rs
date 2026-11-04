use super::{Plan, PlanError, PlanStore};

impl PlanStore {
    pub fn try_get(&self, id: &str) -> Result<Plan, PlanError> {
        self.plan_touches.lock().push(id.into());
        self.load_plan_from_sql(id)?.ok_or(PlanError::NotFound)
    }
    pub(in crate::service_plan) fn load_plan_from_sql(
        &self,
        id: &str,
    ) -> Result<Option<Plan>, PlanError> {
        self.plan_loads.lock().push(id.into());
        self.database.load_plan(id)
    }
    pub fn plan_loads_for_test(&self) -> Vec<String> {
        self.plan_loads.lock().clone()
    }
    pub fn reset_plan_loads_for_test(&self) {
        self.plan_loads.lock().clear();
    }
    pub fn edit_plan_fixture<T>(
        &self,
        id: &str,
        now: u64,
        edit: impl FnOnce(&mut Plan) -> Result<T, PlanError>,
    ) -> Result<T, PlanError> {
        let turn = self.mutation_writer.enter();
        let mut fixture = self.try_get(id)?;
        let now = now.max(fixture.updated_at);
        let output = edit(&mut fixture)?;
        self.import_payload_in_writer(&turn, &mut fixture, now)
            .map_err(|error| PlanError::Io(error.to_string()))?;
        Ok(output)
    }
    #[cfg(test)]
    pub(in crate::service_plan) fn export_sidecars_for_test(&self, id: &str) {
        let plan = self.try_get(id).unwrap();
        self.persist_test_sidecars(&plan, None).unwrap();
    }
    #[cfg(test)]
    pub(in crate::service_plan) fn forget_all_plans_for_test(&self) {
        self.index.write().clear();
    }
}

#[cfg(test)]
mod tests;
