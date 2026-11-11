use codeterm_team_package::contracts::{
    TemplateOperation, TemplateStartRequest, TemplateUnavailable,
};

/// The pair primitive's template branch. Until template start lands it refuses before any state changes.
pub(crate) fn start(_template: &TemplateStartRequest) -> Result<(), String> {
    Err(TemplateUnavailable::new(TemplateOperation::Start).to_string())
}

#[cfg(test)]
mod tests;
