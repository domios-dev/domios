use crate::{graph, scheduler::Handle, storage};
use codeterm_service_core::service::ServiceContext;

pub trait AutomationFilesExt {
    fn automation_node_files(
        &self,
        identity: &graph::Identity,
        bucket: Option<&str>,
        handler: Option<&str>,
    ) -> Result<Vec<graph::files::FileReference>, String>;
}

impl AutomationFilesExt for ServiceContext {
    fn automation_node_files(
        &self,
        identity: &graph::Identity,
        bucket: Option<&str>,
        handler: Option<&str>,
    ) -> Result<Vec<graph::files::FileReference>, String> {
        graph::validate_scope(
            &self.mux.read(),
            &identity.workspace_id,
            identity.team_id.as_deref(),
        )?;
        let handle = self
            .host_extension::<Handle>()
            .ok_or("blueprints not initialized")?;
        let (jobs, _) = storage::committed_scope(
            &handle.store,
            &identity.workspace_id,
            identity.team_id.as_deref(),
        )?;
        let reachable = graph::reachable_ids(&jobs);
        let job = jobs
            .iter()
            .find(|job| graph::identity(job) == *identity && reachable.contains(&job.id))
            .ok_or("Blueprint node is outside the graph scope")?;
        graph::files::discover(job, bucket, handler)
    }
}
