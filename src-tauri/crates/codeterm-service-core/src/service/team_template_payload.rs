use codeterm_team_package::contracts::{
    PayloadApplyResult, TemplateOperation, TemplateUnavailable,
};
use codeterm_team_package::Package;

use super::ServiceContext;

/// Applies the opted-in payload of `package` to a Team at start.
pub fn apply(
    _svc: &ServiceContext,
    _team_id: &str,
    _package: &Package,
    _opt_out: &[String],
) -> Result<PayloadApplyResult, TemplateUnavailable> {
    Err(TemplateUnavailable::new(TemplateOperation::ApplyPayload))
}

/// Collects a Team's payload back into a package at bake.
pub fn collect(_svc: &ServiceContext, _team_id: &str) -> Result<Package, TemplateUnavailable> {
    Err(TemplateUnavailable::new(TemplateOperation::CollectPayload))
}
