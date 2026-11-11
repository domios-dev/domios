use codeterm_team_package::contracts::{TemplateOperation, TemplateUnavailable};
use serde_json::Value;

use super::ServiceContext;

pub fn list(_svc: &ServiceContext) -> Result<Value, TemplateUnavailable> {
    Err(TemplateUnavailable::new(TemplateOperation::List))
}

pub fn preview(
    _svc: &ServiceContext,
    _params: Option<&Value>,
) -> Result<Value, TemplateUnavailable> {
    Err(TemplateUnavailable::new(TemplateOperation::Preview))
}

pub fn start(_svc: &ServiceContext, _params: Option<&Value>) -> Result<Value, TemplateUnavailable> {
    Err(TemplateUnavailable::new(TemplateOperation::Start))
}
