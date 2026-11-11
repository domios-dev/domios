use codeterm_team_package::contracts::{
    TemplateOperation, TemplateStartRequest, TemplateStartResult, TemplateUnavailable,
};

use super::ServiceContext;

/// Admits a mesh-origin template start on this peer Main.
pub fn start_from_peer(
    _svc: &ServiceContext,
    _peer_id: &str,
    _request: &TemplateStartRequest,
) -> Result<TemplateStartResult, TemplateUnavailable> {
    Err(TemplateUnavailable::new(TemplateOperation::RemoteStart))
}
