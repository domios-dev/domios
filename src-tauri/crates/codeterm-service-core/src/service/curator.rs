use super::{AuthenticatedPlanCaller, ServiceContext};
use crate::plan::CuratorGrant;

mod authority;
mod save;
mod skill;
pub use authority::CuratorIdentity;

pub struct CuratorGrantScope {
    pub blueprints: Vec<String>,
    pub plan_skills: Vec<String>,
    pub saves: Vec<String>,
}
