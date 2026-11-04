use crate::plan::coverage::{
    compute_coverage_report, compute_spec_coverage_report, CoverageReport, SpecCoverageReport,
};
use crate::service_plan::{PlanError, PlanStore};

impl PlanStore {
    pub fn coverage_report(&self, id: &str) -> Result<Option<CoverageReport>, PlanError> {
        self.database
            .coverage_input(id)
            .map(|input| input.map(compute_coverage_report))
    }

    pub fn spec_coverage_report(&self, id: &str) -> Result<Option<SpecCoverageReport>, PlanError> {
        self.database
            .spec_coverage_input(id)
            .map(|input| input.map(|(spec, plans)| compute_spec_coverage_report(spec, &plans)))
    }
}
