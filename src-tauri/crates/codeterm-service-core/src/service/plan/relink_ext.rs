use crate::service::plan::relink_to_owner;
use crate::service::ServiceContext;

impl ServiceContext {
    pub fn relink_plan_hierarchy(&self, plan_id: &str, new_owner_tab: &str) -> usize {
        let Some(plan) = self.plan_store.get(plan_id) else {
            return 0;
        };
        let relinked = {
            let mut mux = self.mux.write();
            relink_to_owner(
                &mut crate::service::plan::lane_port_impl::LaneStatePort(&mut *mux),
                &plan,
                new_owner_tab,
            )
        };
        if relinked > 0 {
            self.emit_state_changed();
        }
        relinked
    }
}
