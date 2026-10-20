//! Refusal codes for the "add a member to a Team" verb, defined once so the
//! daemon, the CLI, and the UI name a structural refusal by the same token
//! instead of matching on prose.

/// The Team already holds a live Orchestrator; a second one is refused.
pub const TEAM_ORCHESTRATOR_ALREADY_LIVE: &str = "team_orchestrator_already_live";

/// The requested member role is not one this verb can place.
pub const TEAM_MEMBER_ROLE_UNKNOWN: &str = "team_member_role_unknown";

/// The role takes a parent, and the Team holds no member that can spawn one.
pub const TEAM_ROLE_REQUIRES_SPAWNER: &str = "team_role_requires_spawner";

/// The calling tab may not staff this Team.
pub const TEAM_CALLER_UNAUTHORIZED: &str = "team_caller_unauthorized";

/// The calling tab is a member but is not allowed to spawn.
pub const TEAM_CALLER_CANNOT_SPAWN: &str = "team_caller_cannot_spawn";

/// Neither the caller nor the Team resolves to a workspace to place the member in.
pub const TEAM_WORKSPACE_UNRESOLVED: &str = "team_workspace_unresolved";

/// `--team` was combined with a role that does not anchor a Team.
pub const TEAM_SPAWN_ROLE_MUST_ANCHOR: &str = "team_spawn_role_must_anchor";

/// `--team --role Orchestrator` was combined with a flag the staffing verb does
/// not carry; the flag is named rather than silently dropped.
pub const TEAM_SPAWN_FLAG_UNSUPPORTED: &str = "team_spawn_flag_unsupported";
