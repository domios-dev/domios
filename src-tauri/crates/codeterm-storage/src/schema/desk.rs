/// Desk tables are registered after V1, Cron and Sentinel by the Storage owner.
pub(crate) const SCHEMA: &str = r#"
CREATE TABLE desk_meta (
    singleton INTEGER PRIMARY KEY NOT NULL CHECK (singleton = 1),
    version TEXT NOT NULL,
    timestamp INTEGER NOT NULL CHECK (timestamp >= 0),
    active_workspace_id TEXT,
    revision INTEGER NOT NULL CHECK (revision > 0),
    workspace_order_revision INTEGER NOT NULL CHECK (workspace_order_revision > 0)
) STRICT;
CREATE TABLE desk_workspace_state (
    workspace_id TEXT PRIMARY KEY NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    sort_order INTEGER NOT NULL,
    active_tab_id TEXT,
    revision INTEGER NOT NULL CHECK (revision > 0),
    tab_order_revision INTEGER NOT NULL CHECK (tab_order_revision > 0)
) STRICT;
CREATE TABLE desk_teams (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) > 0),
    ordinal INTEGER NOT NULL UNIQUE CHECK (ordinal >= 0),
    name TEXT,
    lifecycle TEXT NOT NULL CHECK (lifecycle IN ('forming', 'active', 'closing')),
    created_at INTEGER NOT NULL CHECK (created_at >= 0),
    workspace_id TEXT NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0)
) STRICT;
CREATE INDEX desk_teams_by_workspace ON desk_teams(workspace_id);
CREATE TABLE desk_tabs (
    id TEXT PRIMARY KEY NOT NULL CHECK (length(id) > 0),
    workspace_id TEXT NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    title TEXT NOT NULL,
    description TEXT,
    sort_order INTEGER NOT NULL,
    revision INTEGER NOT NULL CHECK (revision > 0)
) STRICT;
CREATE INDEX desk_tabs_by_workspace ON desk_tabs(workspace_id, ordinal);
CREATE TABLE desk_layout_nodes (
    id INTEGER PRIMARY KEY,
    group_id TEXT NOT NULL REFERENCES desk_tabs(id) ON DELETE CASCADE,
    parent_id INTEGER,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    kind TEXT NOT NULL CHECK (kind IN ('leaf', 'split')),
    orientation TEXT,
    ratio REAL,
    tab_id TEXT,
    cwd TEXT,
    fields_json TEXT,
    CHECK ((kind = 'split' AND orientation IS NOT NULL AND ratio IS NOT NULL
            AND tab_id IS NULL AND fields_json IS NOT NULL)
        OR (kind = 'leaf' AND orientation IS NULL AND ratio IS NULL
            AND tab_id IS NOT NULL AND fields_json IS NOT NULL)),
    UNIQUE (id, group_id),
    FOREIGN KEY (parent_id, group_id)
        REFERENCES desk_layout_nodes(id, group_id) ON DELETE CASCADE
) STRICT;
CREATE INDEX desk_layout_by_group ON desk_layout_nodes(group_id, parent_id, ordinal);
CREATE UNIQUE INDEX desk_layout_root ON desk_layout_nodes(group_id) WHERE parent_id IS NULL;
CREATE UNIQUE INDEX desk_layout_child ON desk_layout_nodes(group_id, parent_id, ordinal) WHERE parent_id IS NOT NULL;
CREATE TABLE agent_session_identities (
    provider TEXT NOT NULL CHECK (length(provider) > 0),
    session_id TEXT NOT NULL CHECK (length(session_id) > 0),
    record_provider TEXT NOT NULL CHECK (length(record_provider) > 0),
    role TEXT NOT NULL,
    role_profile TEXT,
    parent_tab_id TEXT,
    can_spawn INTEGER NOT NULL CHECK (can_spawn IN (0, 1)),
    revision INTEGER NOT NULL CHECK (revision > 0),
    PRIMARY KEY (provider, session_id)
) STRICT;
"#;
