use codeterm_team_package::contracts::{TemplateOperation, TemplateUnavailable};
use serde_json::Value;

use super::ServiceContext;

pub fn recall(
    _svc: &ServiceContext,
    _params: Option<&Value>,
) -> Result<Value, TemplateUnavailable> {
    Err(TemplateUnavailable::new(TemplateOperation::Recall))
}
