use codeterm_team_package::contracts::{TemplateOperation, TemplateUnavailable};
use serde_json::Value;

use super::ServiceContext;

pub fn distill(
    _svc: &ServiceContext,
    _params: Option<&Value>,
) -> Result<Value, TemplateUnavailable> {
    Err(TemplateUnavailable::new(TemplateOperation::Distill))
}

pub fn export(
    _svc: &ServiceContext,
    _params: Option<&Value>,
) -> Result<Value, TemplateUnavailable> {
    Err(TemplateUnavailable::new(TemplateOperation::Export))
}

pub fn import(
    _svc: &ServiceContext,
    _params: Option<&Value>,
) -> Result<Value, TemplateUnavailable> {
    Err(TemplateUnavailable::new(TemplateOperation::Import))
}
