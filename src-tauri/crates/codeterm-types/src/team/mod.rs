use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Team lifecycle. Shaped to later support ephemeral orchestration; only manual
/// transitions ship in this phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TeamLifecycle {
    #[default]
    Forming,
    Active,
    Paused,
    Complete,
    Archived,
}

/// A Team groups the panes of one multi-agent effort. Membership is the data
/// relation `Pane.team_id` + role — there is no separate membership table. Bound
/// 1:1 to a plan (Phase 3); minted at the spawn edge (Phase 2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Team {
    /// Stable id, `team_` prefix.
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default)]
    pub lifecycle: TeamLifecycle,
    pub created_at: u64,
    pub workspace_id: String,
}

impl Team {
    /// Mint a fresh forming team in the given workspace. `created_at` is a caller
    /// supplied epoch-ms stamp so this stays pure and testable.
    pub fn new(workspace_id: impl Into<String>, created_at: u64) -> Self {
        Self {
            id: Self::mint_id(),
            name: None,
            lifecycle: TeamLifecycle::Forming,
            created_at,
            workspace_id: workspace_id.into(),
        }
    }

    /// Generate a stable `team_`-prefixed id.
    pub fn mint_id() -> String {
        let bytes = Uuid::new_v4();
        let bytes = bytes.as_bytes();
        format!(
            "team_{:02x}{:02x}{:02x}{:02x}",
            bytes[0], bytes[1], bytes[2], bytes[3]
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn team_round_trips_through_serde() {
        let team = Team {
            id: "team_abcd1234".into(),
            name: Some("Alpha".into()),
            lifecycle: TeamLifecycle::Active,
            created_at: 1_700_000_000_000,
            workspace_id: "ws_1".into(),
        };
        let v = serde_json::to_value(&team).unwrap();
        assert_eq!(v["lifecycle"], "active");
        let back: Team = serde_json::from_value(v).unwrap();
        assert_eq!(back, team);
    }

    #[test]
    fn legacy_team_json_without_optional_fields_defaults() {
        // A minimal record (no name, no lifecycle) must deserialize with a
        // forming default and no name.
        let back: Team =
            serde_json::from_str(r#"{"id":"team_x","created_at":0,"workspace_id":"ws"}"#).unwrap();
        assert_eq!(back.name, None);
        assert_eq!(back.lifecycle, TeamLifecycle::Forming);
    }

    #[test]
    fn minted_id_carries_team_prefix() {
        assert!(Team::new("ws", 0).id.starts_with("team_"));
    }
}
