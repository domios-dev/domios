use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AttemptBindingRefusalKind {
    CallerTabMissing,
    CallerNonceMissing,
    CallerTabNotFound,
    WorkerRoleRequired,
    LivePtyRequired,
    CurrentBindingRequired,
    CallerSessionMismatch,
    ProviderMissing,
    TargetMismatch,
    TeamMissing,
    TeamMismatch,
    AttemptNotUnique,
    AttemptNonceMissing,
    LineageInputInvalid,
    LineageContainsCurrentNonce,
    LineageOldNonceNotUnique,
    LineageSessionMismatch,
    LineageNonceInvalid,
    LineageSuffixUnqualified,
    LineageEncodingFailed,
    PlanOrRequestInvalid,
    TaskOwnershipMismatch,
    StaleTaskEpoch,
    StaleAttemptEpoch,
    AttemptAuthorityInvalid,
    ReceiptInvalid,
}

impl AttemptBindingRefusalKind {
    pub fn refusal(self) -> super::PlanError {
        super::PlanError::AttemptBindingRestoreRefused {
            kind: self,
            reason: self.reason().into(),
        }
    }

    fn reason(self) -> &'static str {
        match self {
            Self::CallerTabMissing => "caller must name its own current tab",
            Self::CallerNonceMissing => "caller must supply its current binding nonce",
            Self::CallerTabNotFound => "caller tab is not present",
            Self::WorkerRoleRequired => "caller must hold a current Worker leaf role",
            Self::LivePtyRequired => "caller must have a live local PTY agent",
            Self::CurrentBindingRequired => "issued and provider-reported nonce must match the caller nonce",
            Self::CallerSessionMismatch => "supplied session must match the nonce-authenticated tab session",
            Self::ProviderMissing => "current tab must identify its provider",
            Self::TargetMismatch => "exact task must belong to the requested plan",
            Self::TeamMissing => "current Worker must belong to a registered Team",
            Self::TeamMismatch => "current Worker Team must match the plan Team",
            Self::AttemptNotUnique => "target must identify exactly one attempt",
            Self::AttemptNonceMissing => "attempt must retain its prior binding nonce",
            Self::LineageInputInvalid => "lineage proof requires nonempty session and nonces",
            Self::LineageContainsCurrentNonce => "retained lineage must not contain the current nonce",
            Self::LineageOldNonceNotUnique => "retained lineage must contain the old nonce exactly once",
            Self::LineageSessionMismatch => "old nonce must be bound to the current session",
            Self::LineageNonceInvalid => "retained suffix nonces must be nonempty and unique",
            Self::LineageSuffixUnqualified => "every retained suffix entry must belong to the current session or its resume intent",
            Self::LineageEncodingFailed => "retained lineage could not be encoded",
            Self::PlanOrRequestInvalid => "plan must be active and request identity and Team must be complete and matching",
            Self::TaskOwnershipMismatch => "task must have the exact local assignment, lane, status and owner session without an interruption",
            Self::StaleTaskEpoch => "request owner epoch must equal the current task owner epoch",
            Self::StaleAttemptEpoch => "attempt owner epoch must equal the current task owner epoch",
            Self::AttemptAuthorityInvalid => "attempt must be an acknowledged local Worker with matching provider and session and eligible continuation",
            Self::ReceiptInvalid => "old nonce and unique restoration receipt must match the current attempt",
        }
    }
}
