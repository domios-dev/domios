use codeterm_team_package::contracts::{ModelDecision, TemplateOperation, TemplateUnavailable};
use codeterm_team_package::ModelPreference;

use super::ServiceContext;

/// Decides one role's model on this machine from its preference.
pub fn decide(
    _svc: &ServiceContext,
    _preference: Option<&ModelPreference>,
    _effort: Option<&str>,
) -> Result<ModelDecision, TemplateUnavailable> {
    Err(TemplateUnavailable::new(TemplateOperation::ModelDecision))
}
