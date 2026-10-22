use codeterm_blueprint_types::blueprint_document::{
    checker::{DuplicateFinding, DuplicateOracle, DuplicateStatus},
    projection, Document,
};
use codeterm_service_core::service::ServiceContext;

pub(super) struct ScopedDuplicates<'a> {
    pub service: &'a ServiceContext,
    pub workspace: &'a str,
}
impl DuplicateOracle for ScopedDuplicates<'_> {
    fn compare(&self, document: &Document) -> DuplicateStatus {
        match compare(self.service, self.workspace, document) {
            Ok(status) => status,
            Err(reason) => DuplicateStatus::Unavailable(format!(
                "Scoped vector/WL comparison unavailable: {reason}"
            )),
        }
    }
}
fn compare(
    service: &ServiceContext,
    workspace: &str,
    document: &Document,
) -> Result<DuplicateStatus, String> {
    let area = format!("blueprints:{workspace}");
    let identity = projection::identity(document)?;
    let entries = service.mem.list_blueprint_solutions(&area)?;
    let Some(entry) = entries
        .into_iter()
        .find(|entry| projection::identity(&entry.document).is_ok_and(|hash| hash == identity))
    else {
        return Ok(DuplicateStatus::Unavailable("Vector/WL comparison requires an indexed solution identity; this unsaved graph has none".into()));
    };
    let twins = service
        .mem
        .blueprint_twins(&area, &entry.id, &Default::default())?;
    Ok(DuplicateStatus::Available(
        twins
            .into_iter()
            .filter_map(|(other, comparison)| {
                let verdict = serde_json::to_value(comparison.verdict)
                    .ok()?
                    .as_str()?
                    .to_owned();
                if verdict == "distinct" {
                    return None;
                }
                Some(DuplicateFinding {
                    verdict: verdict.into(),
                    other,
                    why: comparison.why,
                })
            })
            .collect(),
    ))
}
