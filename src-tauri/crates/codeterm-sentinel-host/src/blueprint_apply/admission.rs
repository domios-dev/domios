use codeterm_blueprint_types::blueprint_document::{
    checker::{DuplicateFinding, DuplicateOracle, DuplicateStatus},
    Document,
};
use codeterm_mem::blueprint_library::dedupe::{compare_documents, Verdict};
use codeterm_service_core::service::ServiceContext;

pub(super) struct ScopedDuplicates<'a> {
    pub service: &'a ServiceContext,
    pub workspace: &'a str,
    pub team: &'a Option<String>,
    pub updating: Option<&'a str>,
    pub recipe: Option<&'a str>,
}
impl DuplicateOracle for ScopedDuplicates<'_> {
    fn compare(&self, document: &Document) -> DuplicateStatus {
        if document.nodes().count() > 64
            || serde_json::to_vec(document).map_or(true, |bytes| {
                bytes.len() > codeterm_blueprint_types::blueprint_recipe::MAX_RECIPE_BYTES
            })
        {
            return DuplicateStatus::Unavailable(
                "Candidate exceeds admission bounds; duplicate comparison was not run".into(),
            );
        }
        let mut findings = Vec::new();
        let mut unavailable = vec![
            "Candidate vectors unavailable: near/semantic duplicate comparison has no verdict"
                .to_string(),
        ];
        match self
            .service
            .mem
            .blueprint_solutions_readonly(&format!("blueprints:{}", self.workspace))
        {
            Ok(entries) => {
                for entry in entries
                    .into_iter()
                    .filter(|entry| Some(entry.id.as_str()) != self.recipe)
                {
                    compare(document, &entry.document, entry.id, &mut findings);
                }
            }
            Err(reason) => {
                unavailable.push(format!("Saved-library comparison unavailable: {reason}"))
            }
        }
        if let Some(handle) = self.service.host_extension::<crate::scheduler::Handle>() {
            let jobs = handle
                .store
                .lock()
                .jobs
                .iter()
                .filter(|job| {
                    job.workspace_id == self.workspace
                        && &job.team_id == self.team
                        && self
                            .updating
                            .map_or(job.name.as_deref() != Some(document.name.as_str()), |id| {
                                job.id != id
                            })
                })
                .map(|job| (job.id.clone(), job.document.clone()))
                .collect::<Vec<_>>();
            for (id, saved) in jobs {
                match saved
                    .as_ref()
                    .ok_or("missing native document".to_string())
                    .and_then(Document::from_definition)
                {
                    Ok(other) => compare(document, &other, id, &mut findings),
                    Err(reason) => {
                        unavailable.push(format!("Live Blueprint {id} unavailable: {reason}"))
                    }
                }
            }
        } else {
            unavailable.push("Live Team comparison unavailable: Blueprint store absent".into());
        }
        DuplicateStatus::Partial {
            findings,
            reason: unavailable.join("; "),
        }
    }
}
fn compare(
    document: &Document,
    other: &Document,
    id: String,
    findings: &mut Vec<DuplicateFinding>,
) {
    let comparison = compare_documents(document, other);
    let verdict = match comparison.verdict {
        Verdict::Exact => "exact",
        Verdict::StructuralTwin => "structural_twin",
        _ => return,
    };
    findings.push(DuplicateFinding {
        verdict: verdict.into(),
        other: id,
        why: comparison.why,
    });
}
