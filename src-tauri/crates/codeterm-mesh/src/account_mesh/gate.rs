use super::ledger::{LinkAdmission, LinkRecord};
use super::ports::{AccountMeshState, AccountPresence};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
    AccountPending,
    SignedOut,
    NotSignedIn,
    OtherAccount,
    NoProvenance,
    KeyMismatch,
    Revoked,
    LocalMembershipExpired,
    PeerMembershipMissing,
    PeerMembershipExpired,
    PeerUnavailable,
    /// The account server refused this machine a slot under the plan's machine limit.
    PlanLimitLocked,
}

impl BlockReason {
    pub fn code(self) -> &'static str {
        match self {
            Self::PlanLimitLocked => codeterm_entitlements::PLAN_LIMIT_LOCKED,
            _ => "blocked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrafficDecision {
    Open,
    Blocked(BlockReason),
}

impl TrafficDecision {
    pub fn is_open(self) -> bool {
        self == Self::Open
    }
}

/// Decides whether one paired edge may carry protected traffic right now.
/// Pure: every input is explicit so the same rule serves inbound, outbound and streams.
pub fn decide(
    record: Option<&LinkRecord>,
    peer_public_key: &str,
    state: &AccountMeshState,
    now: u64,
) -> TrafficDecision {
    use BlockReason::*;
    use TrafficDecision::Blocked;

    if state.presence == AccountPresence::SignedOut {
        return Blocked(SignedOut);
    }
    let Some(record) = record else {
        return Blocked(NoProvenance);
    };
    if record.admission.public_key() != peer_public_key {
        return Blocked(KeyMismatch);
    }
    match &record.admission {
        LinkAdmission::Account {
            account_id,
            installation_id,
            ..
        } => {
            match state.signed_in_account() {
                None => return Blocked(NotSignedIn),
                Some(current) if current != account_id.as_str() => return Blocked(OtherAccount),
                Some(_) => {}
            }
            if state.revoked_installations.contains(installation_id) {
                return Blocked(Revoked);
            }
            if state.unavailable_installations.contains(installation_id) {
                return Blocked(PeerUnavailable);
            }
            if state.current_membership(now).is_none() {
                return Blocked(if state.plan_limit_locked {
                    PlanLimitLocked
                } else {
                    LocalMembershipExpired
                });
            }
            let Some(remote) = record.remote_membership.as_ref() else {
                return Blocked(PeerMembershipMissing);
            };
            let matches = &remote.verified.account_id == account_id
                && &remote.verified.installation_id == installation_id;
            if !matches || !remote.verified.is_current(now) {
                return Blocked(PeerMembershipExpired);
            }
            TrafficDecision::Open
        }
    }
}

/// Earliest signed boundary at which an open account connection must be re-decided.
pub fn admission_deadline(record: Option<&LinkRecord>, state: &AccountMeshState) -> Option<u64> {
    let record = record?;
    let local = state.membership.as_ref().map(|m| m.verified.expires_at);
    let remote = record
        .remote_membership
        .as_ref()
        .map(|m| m.verified.expires_at);
    match (local, remote) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}
