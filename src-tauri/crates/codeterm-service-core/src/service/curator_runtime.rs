use crate::service::ServiceContext;
use std::collections::BTreeSet;
#[derive(Default)]
pub struct CuratorTabs(pub parking_lot::RwLock<std::collections::BTreeMap<String, String>>);
pub fn curator_bootstrap_ids(
    store: &crate::service::plan::PlanStore,
) -> Result<BTreeSet<String>, crate::service::plan::PlanError> {
    Ok(store.curator_bootstrap_plan_ids()?.into_iter().collect())
}
impl ServiceContext {
    pub fn curator_now(&self) -> Result<u64, String> {
        self.extensions
            .try_get::<crate::CronState>()
            .map(|clock| clock.lock().now_ms())
            .ok_or_else(|| "Curator clock is unavailable".into())
    }
}
