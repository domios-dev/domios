use crate::service::plan::relink_selected_to_owner;
use crate::service::MuxEmitCause;
use crate::service::ServiceContext;

impl ServiceContext {
    pub fn relink_plan_hierarchy(
        &self,
        plan_id: &str,
        new_owner_tab: &str,
    ) -> Result<usize, String> {
        let targets = match self.plan_store.relink_targets_projection(plan_id, true) {
            Ok(targets) => targets,
            Err(crate::service::plan::PlanError::NotFound) => return Ok(0),
            Err(error) => return Err(error.to_string()),
        };
        let relinked = {
            let mut mux = self.mux.write();
            relink_selected_to_owner(
                &mut crate::service::plan::lane_port_impl::LaneStatePort(&mut *mux),
                &targets,
                new_owner_tab,
            )
        };
        if relinked > 0 {
            self.emit_state_changed(MuxEmitCause::PlanTask);
        }
        Ok(relinked)
    }
}
