use codeterm_team_package::contracts::{TemplateOperation, TemplateUnavailable};
use serde_json::Value;

use super::ServiceContext;

/// The role types registered for a Team.
pub fn list(_svc: &ServiceContext, _team_id: &str) -> Result<Value, TemplateUnavailable> {
    Err(TemplateUnavailable::new(TemplateOperation::Roles))
}
