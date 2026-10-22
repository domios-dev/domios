use crate::{amend, graph, scheduler::Handle, storage};
use codeterm_blueprint_types::blueprint_document::{calls, Document};
use codeterm_blueprint_types::blueprint_project;
use codeterm_service_core::service::ServiceContext;
use serde_json::{json, Value};

pub trait BlueprintViewerExt {
    fn blueprint_viewer_history(
        &self,
        workspace: &str,
        team: Option<&str>,
        id: &str,
    ) -> Result<Value, String>;
    fn blueprint_project_host(&self, workspace: &str, path: Option<&str>) -> Result<Value, String>;
}
impl BlueprintViewerExt for ServiceContext {
    fn blueprint_viewer_history(
        &self,
        workspace: &str,
        team: Option<&str>,
        id: &str,
    ) -> Result<Value, String> {
        graph::validate_scope(&self.mux.read(), workspace, team)?;
        let handle = self
            .host_extension::<Handle>()
            .ok_or("blueprints not initialized")?;
        let (job, history) = {
            let store = handle.store.lock();
            store.ensure_available()?;
            let job = store
                .get(id)
                .filter(|job| job.workspace_id == workspace && job.team_id.as_deref() == team)
                .cloned()
                .ok_or("Blueprint not found in this scope")?;
            (job, store.typed_history(id)?)
        };
        let history = match history {
            Some(history) => {
                let records = history
                    .wait(storage::COMMIT_DEADLINE)
                    .map_err(|e| e.to_string())?
                    .value;
                amend::from_storage_history(job.id, storage::history_versions(records))
            }
            None => amend::history(&job),
        };
        history_view(&history)
    }
    fn blueprint_project_host(&self, workspace: &str, path: Option<&str>) -> Result<Value, String> {
        let root = {
            let mux = self.mux.read();
            mux.get_workspace(workspace)
                .and_then(|workspace| workspace.cwd.clone())
                .ok_or("Workspace repository unavailable")?
        };
        let value = match path {
            Some(path) => {
                serde_json::to_value(blueprint_project::show(std::path::Path::new(&root), path)?)
            }
            None => serde_json::to_value(blueprint_project::list(std::path::Path::new(&root))?),
        }
        .map_err(|e| e.to_string())?;
        Ok(calls::projection::bounded(&value, 0))
    }
}
fn history_view(history: &amend::History) -> Result<Value, String> {
    let mut versions = Vec::new();
    let retained = history.versions.len().saturating_sub(12);
    for version in &history.versions[retained..] {
        let previous = version.amendment.as_ref().and_then(|amendment| {
            history
                .versions
                .iter()
                .find(|candidate| candidate.version == amendment.replaced_version)
        });
        let after: Document = serde_json::from_value(
            version
                .definition
                .document
                .clone()
                .ok_or("Native definition unavailable")?,
        )
        .map_err(|e| e.to_string())?;
        let after = calls::FrozenClosure::from_document(after)?;
        let before = previous
            .map(|before| {
                let document: Document = serde_json::from_value(
                    before
                        .definition
                        .document
                        .clone()
                        .ok_or("Previous native definition unavailable")?,
                )
                .map_err(|e| e.to_string())?;
                calls::FrozenClosure::from_document(document)
            })
            .transpose()?;
        let diff = before
            .as_ref()
            .map(|before| calls::projection::diff(before, &after));
        versions.push(json!({"version":version.version,"replaces":version.amendment.as_ref().map(|a| a.replaced_version),"diff":diff,"before":before.as_ref().map(calls::projection::graph).transpose()?,"after":calls::projection::graph(&after)?}));
    }
    Ok(
        json!({"current_version":history.current_version,"history_truncated":history.history_truncated || retained > 0,"versions":versions}),
    )
}

#[cfg(test)]
mod tests;
