use axum::{
    extract::{Path, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use super::hook_routes::finish_session_binding;
use super::routes::{resolve_tab, AppState};
use super::session_binding_routes::VerifiedRebindOutcome;
use crate::service::tab_repair::{
    TabRepairError, TabRepairErrorKind, TabRepairResponse, TabRepairSource,
    TAB_REPAIR_VERIFIED_SESSION_REQUIRED,
};
use crate::session::lifecycle::AgentTypeWriteOrigin;

const TAB_REPAIR_INVALID_REQUEST: &str = "tab_repair_invalid_request";

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TabRepairRequest {
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    from_evidence: bool,
    #[serde(default)]
    from_journal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TabRepairMode {
    Provider(String),
    Session(String),
    LiveEvidence,
    Journal,
}

impl TabRepairRequest {
    fn mode(self) -> Result<TabRepairMode, ApiRepairError> {
        let provider_present = self.provider.is_some();
        let session_present = self.session_id.is_some();
        let provider = nonempty(self.provider);
        let session = nonempty(self.session_id);
        let selections = usize::from(provider.is_some())
            + usize::from(session.is_some())
            + usize::from(self.from_evidence)
            + usize::from(self.from_journal);
        if selections > 1 {
            return Err(ApiRepairError::new(
                StatusCode::BAD_REQUEST,
                TAB_REPAIR_INVALID_REQUEST,
                "choose exactly one of provider, session_id, from_evidence, or from_journal",
            ));
        }
        if provider_present && provider.is_none() {
            return Err(ApiRepairError::new(
                StatusCode::BAD_REQUEST,
                TAB_REPAIR_INVALID_REQUEST,
                "provider cannot be empty",
            ));
        }
        if session_present && session.is_none() {
            return Err(ApiRepairError::new(
                StatusCode::BAD_REQUEST,
                TAB_REPAIR_INVALID_REQUEST,
                "session_id cannot be empty",
            ));
        }
        Ok(match (provider, session, self.from_journal) {
            (Some(provider), _, _) => TabRepairMode::Provider(provider),
            (_, Some(session), _) => TabRepairMode::Session(session),
            (_, _, true) => TabRepairMode::Journal,
            _ => TabRepairMode::LiveEvidence,
        })
    }
}

fn nonempty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

#[derive(Debug)]
struct ApiRepairError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ApiRepairError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    fn response(self) -> (StatusCode, Json<serde_json::Value>) {
        let could_not_verify = serde_json::json!({
            "status": "could_not_verify",
            "previous": null,
            "current": null,
        });
        (
            self.status,
            Json(serde_json::json!({
                "ok": false,
                "error": {"code": self.code, "message": self.message},
                "provider": could_not_verify.clone(),
                "session": could_not_verify.clone(),
                "parser_lock": could_not_verify,
            })),
        )
    }
}

impl From<TabRepairError> for ApiRepairError {
    fn from(error: TabRepairError) -> Self {
        let status = match error.kind {
            TabRepairErrorKind::NotFound => StatusCode::NOT_FOUND,
            TabRepairErrorKind::NotAuthorized => StatusCode::FORBIDDEN,
            TabRepairErrorKind::LivePtyRequired => StatusCode::BAD_REQUEST,
            TabRepairErrorKind::VerifiedSessionRequired
            | TabRepairErrorKind::EvidenceChanged
            | TabRepairErrorKind::JournalSourceMissing => StatusCode::CONFLICT,
            TabRepairErrorKind::JournalUnavailable => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self::new(status, error.code(), error.message)
    }
}

type RepairResult = Result<Json<TabRepairResponse>, (StatusCode, Json<serde_json::Value>)>;

pub(crate) async fn repair_tab(
    AxumState(state): AxumState<AppState>,
    Path(raw_tab_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<TabRepairRequest>,
) -> RepairResult {
    let result = repair_tab_inner(state, raw_tab_id, headers, request).await;
    result.map(Json).map_err(ApiRepairError::response)
}

async fn repair_tab_inner(
    state: AppState,
    raw_tab_id: String,
    headers: HeaderMap,
    request: TabRepairRequest,
) -> Result<TabRepairResponse, ApiRepairError> {
    let tab_id = resolve_tab(&state, &raw_tab_id).map_err(|(status, message)| {
        ApiRepairError::new(status, "tab_repair_target_not_found", message)
    })?;
    let mode = request.mode()?;
    let caller = crate::api::tab_routes::authenticated_plan_caller(&headers);
    let target = state
        .svc
        .authorize_tab_repair(&tab_id, &caller)
        .map_err(ApiRepairError::from)?;
    match mode {
        TabRepairMode::Provider(provider) => state
            .svc
            .repair_tab_from_live_evidence(target, &caller, Some(&provider))
            .await
            .map_err(ApiRepairError::from),
        TabRepairMode::LiveEvidence => state
            .svc
            .repair_tab_from_live_evidence(target, &caller, None)
            .await
            .map_err(ApiRepairError::from),
        TabRepairMode::Journal => state
            .svc
            .repair_tab_from_journal(target, &caller)
            .await
            .map_err(ApiRepairError::from),
        TabRepairMode::Session(session_id) => {
            repair_tab_from_session(&state, target, &caller, &session_id).await
        }
    }
}

async fn repair_tab_from_session(
    state: &AppState,
    target: crate::service::tab_repair::AuthorizedRepairTarget,
    caller: &crate::service::AuthenticatedPlanCaller,
    session_id: &str,
) -> Result<TabRepairResponse, ApiRepairError> {
    let commit = match super::session_binding_routes::validate_and_commit_rebind(
        &state.svc,
        &target.tab_id,
        session_id,
        caller.tab_id.as_deref(),
        AgentTypeWriteOrigin::Repair,
    )
    .await
    {
        Ok(VerifiedRebindOutcome::Committed(commit)) => commit,
        Ok(VerifiedRebindOutcome::EvidenceChanged) => {
            return Err(TabRepairError::new(
                TabRepairErrorKind::EvidenceChanged,
                "tab session evidence changed before the repair rebind committed",
            )
            .into());
        }
        Err((status, message, _)) => {
            let code = if status == StatusCode::NOT_FOUND {
                "tab_repair_target_not_found"
            } else {
                TAB_REPAIR_VERIFIED_SESSION_REQUIRED
            };
            return Err(ApiRepairError::new(status, code, message));
        }
    };
    let nonce_verified = commit.claim.identity_evidence().nonce_verified();
    let _ = finish_session_binding(
        &state.svc,
        &target.tab_id,
        &commit.provider_id,
        session_id,
        commit.binding_nonce.as_deref(),
        None,
        "cli-repair",
        &serde_json::Value::Null,
        commit.claim,
        crate::session::lifecycle::BindingSource::Hook,
        false,
    )
    .await
    .map_err(|(status, message)| {
        ApiRepairError::new(status, TAB_REPAIR_VERIFIED_SESSION_REQUIRED, message)
    })?;
    let response = TabRepairResponse::from_identity_write(
        target.tab_id,
        TabRepairSource::Session,
        commit.previous_session_id,
        session_id.to_string(),
        commit.provider_id,
        nonce_verified,
        commit.identity_write,
        None,
    )
    .map_err(ApiRepairError::from)?;
    state.svc.emit_state_changed();
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_request_defaults_to_live_evidence() {
        assert_eq!(
            TabRepairRequest::default().mode().unwrap(),
            TabRepairMode::LiveEvidence
        );
    }

    #[test]
    fn repair_modes_are_mutually_exclusive_at_the_api_boundary() {
        let request = TabRepairRequest {
            provider: Some("codex".into()),
            from_journal: true,
            ..Default::default()
        };

        let error = request.mode().unwrap_err();

        assert_eq!(error.code, TAB_REPAIR_INVALID_REQUEST);
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn empty_assertions_are_refused_instead_of_becoming_live_evidence() {
        let request = TabRepairRequest {
            session_id: Some("  ".into()),
            ..Default::default()
        };

        assert_eq!(request.mode().unwrap_err().code, TAB_REPAIR_INVALID_REQUEST);
    }
}
