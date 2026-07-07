use crate::service::plan::relink_to_owner;
use crate::service::ServiceContext;

impl ServiceContext {
    pub fn relink_plan_hierarchy(&self, plan_id: &str, new_owner_pane: &str) -> usize {
        let Some(plan) = self.plan_store.get(plan_id) else {
            return 0;
        };
        let relinked = {
            let mut mux = self.mux.write();
            relink_to_owner(&mut *mux, &plan, new_owner_pane)
        };
        if relinked > 0 {
            self.emit_state_changed();
        }
        relinked
    }
}
