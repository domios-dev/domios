//! Lifecycle-owned provenance for a pair-minted forming Team and the launch
//! attempts reserved with it. The tuple `(creator_tab_id, team_id,
//! child_tab_id, attempt_id)` is the only authority for holding a supervised
//! startup and for reaping the Team the pair itself minted.

use codeterm_types::team::TeamLifecycle;
use serde_json::{json, Value};

use crate::multiplexer::state::MultiplexerState;

/// The exact Team a pair mint owns plus the launch tabs reserved with it. A
/// Team the claimed tab already belonged to is adopted, never leased.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StartupTeamLease {
    pub team_id: String,
    pub creator_tab_id: String,
    pub member_tab_ids: Vec<String>,
}

/// Recorded ownership of one supervised startup attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StartupAuthority {
    pub creator_tab_id: String,
    pub child_tab_id: String,
    pub attempt_id: String,
    pub team_id: Option<String>,
}

impl StartupAuthority {
    pub(crate) fn new(creator_tab_id: &str, child_tab_id: &str, attempt_id: &str) -> Self {
        Self {
            creator_tab_id: creator_tab_id.to_string(),
            child_tab_id: child_tab_id.to_string(),
            attempt_id: attempt_id.to_string(),
            team_id: None,
        }
    }

    pub(crate) fn in_team(mut self, team_id: Option<String>) -> Self {
        self.team_id = team_id;
        self
    }

    pub(crate) fn provenance(&self) -> Value {
        json!({
            "creator_tab_id": self.creator_tab_id,
            "child_tab_id": self.child_tab_id,
            "attempt_id": self.attempt_id,
            "team_id": self.team_id,
        })
    }
}

/// The child tab identity a hold is checked against, read from the mux in one pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ChildStartupState {
    pub attempt_id: Option<String>,
    pub team_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StartupHoldRefusal {
    UnknownChild,
    NoSupervisedAttempt,
    CreatorMismatch {
        creator_tab_id: String,
    },
    AttemptReplaced {
        current_attempt_id: Option<String>,
    },
    TeamMismatch {
        leased_team_id: String,
        child_team_id: Option<String>,
    },
}

impl StartupHoldRefusal {
    pub(crate) fn reason(&self) -> &'static str {
        match self {
            Self::UnknownChild => "unknown_child_tab",
            Self::NoSupervisedAttempt => "no_supervised_attempt",
            Self::CreatorMismatch { .. } => "creator_mismatch",
            Self::AttemptReplaced { .. } => "attempt_replaced",
            Self::TeamMismatch { .. } => "team_mismatch",
        }
    }

    pub(crate) fn message(
        &self,
        caller_tab_id: &str,
        child_tab_id: &str,
        attempt_id: &str,
    ) -> String {
        let detail = match self {
            Self::UnknownChild => format!("Tab {child_tab_id} does not exist"),
            Self::NoSupervisedAttempt => {
                format!("no supervised startup attempt is recorded for Tab {child_tab_id}")
            }
            Self::CreatorMismatch { creator_tab_id } => {
                format!("only creator Tab {creator_tab_id} may hold this attempt")
            }
            Self::AttemptReplaced { current_attempt_id } => format!(
                "attempt {attempt_id} was replaced by {}",
                current_attempt_id.as_deref().unwrap_or("no live attempt")
            ),
            Self::TeamMismatch {
                leased_team_id,
                child_team_id,
            } => format!(
                "attempt is leased to Team {leased_team_id} but Tab {child_tab_id} is in {}",
                child_team_id.as_deref().unwrap_or("no Team")
            ),
        };
        format!(
            "Tab {caller_tab_id} cannot hold startup attempt {attempt_id} for Tab {child_tab_id}: {detail}"
        )
    }

    pub(crate) fn provenance(
        &self,
        caller_tab_id: &str,
        child_tab_id: &str,
        attempt_id: &str,
        supervised: Option<&StartupAuthority>,
    ) -> Value {
        json!({
            "held": false,
            "reason": self.reason(),
            "caller_tab_id": caller_tab_id,
            "child_tab_id": child_tab_id,
            "attempt_id": attempt_id,
            "authority": supervised.map(StartupAuthority::provenance),
        })
    }
}

/// A refused hold, carried to the transport with its provenance intact.
#[derive(Debug, Clone)]
pub(crate) struct StartupHoldRejection {
    pub message: String,
    pub provenance: Value,
}

/// The single authorization rule: caller, child tab, minted Team and current
/// attempt must match the recorded ownership. No role bypasses it.
pub(crate) fn authorize_startup_hold(
    caller_tab_id: &str,
    child_tab_id: &str,
    attempt_id: &str,
    supervised: Option<&StartupAuthority>,
    child: Option<&ChildStartupState>,
) -> Result<StartupAuthority, StartupHoldRefusal> {
    let Some(child) = child else {
        return Err(StartupHoldRefusal::UnknownChild);
    };
    let Some(supervised) = supervised.filter(|authority| authority.child_tab_id == child_tab_id)
    else {
        return Err(StartupHoldRefusal::NoSupervisedAttempt);
    };
    if supervised.creator_tab_id != caller_tab_id {
        return Err(StartupHoldRefusal::CreatorMismatch {
            creator_tab_id: supervised.creator_tab_id.clone(),
        });
    }
    if supervised.attempt_id != attempt_id || child.attempt_id.as_deref() != Some(attempt_id) {
        return Err(StartupHoldRefusal::AttemptReplaced {
            current_attempt_id: child.attempt_id.clone(),
        });
    }
    if let Some(leased_team_id) = supervised.team_id.as_deref() {
        if child.team_id.as_deref() != Some(leased_team_id) {
            return Err(StartupHoldRefusal::TeamMismatch {
                leased_team_id: leased_team_id.to_string(),
                child_team_id: child.team_id.clone(),
            });
        }
    }
    Ok(supervised.clone())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TeamReapOutcome {
    Reaped,
    TeamGone,
    NotForming,
    MemberStillLive,
    UnownedMember,
}

/// Remove the exact reserved Team only while it is forming, empty and free of
/// any member the reservation never claimed. Every other state is a no-op.
pub(crate) fn reap_owned_forming_team(
    mux: &mut MultiplexerState,
    team_id: &str,
    member_tab_ids: &[String],
) -> TeamReapOutcome {
    let Some(team) = mux.get_team(team_id) else {
        return TeamReapOutcome::TeamGone;
    };
    if team.lifecycle != TeamLifecycle::Forming {
        return TeamReapOutcome::NotForming;
    }
    let members = mux.team_member_tab_ids(team_id);
    if members
        .iter()
        .any(|tab_id| !member_tab_ids.contains(tab_id))
    {
        return TeamReapOutcome::UnownedMember;
    }
    if !members.is_empty() {
        return TeamReapOutcome::MemberStillLive;
    }
    mux.remove_team(team_id);
    TeamReapOutcome::Reaped
}

/// Roll back a failed launch: detach the reserved tabs, then apply the same
/// predicate. A tab that has since joined another Team keeps its membership.
pub(crate) fn release_owned_reservation(
    mux: &mut MultiplexerState,
    team_id: &str,
    member_tab_ids: &[String],
) -> TeamReapOutcome {
    for tab_id in member_tab_ids {
        if let Some(tab) = mux.get_tab_mut(tab_id) {
            if tab.team_id.as_deref() == Some(team_id) {
                tab.team_id = None;
            }
        }
    }
    reap_owned_forming_team(mux, team_id, member_tab_ids)
}

/// Sweep the caller's live lease records after a close, borrowed and never
/// owned, so the caller can decide and consume them under one lock.
pub(crate) fn reap_orphaned_leases<'a>(
    mux: &mut MultiplexerState,
    leases: impl IntoIterator<Item = &'a StartupTeamLease>,
) -> Vec<String> {
    leases
        .into_iter()
        .filter(|lease| {
            reap_owned_forming_team(mux, &lease.team_id, &lease.member_tab_ids)
                == TeamReapOutcome::Reaped
        })
        .map(|lease| lease.team_id.clone())
        .collect()
}

#[cfg(test)]
mod tests;
