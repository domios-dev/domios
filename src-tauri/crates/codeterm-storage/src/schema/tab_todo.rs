pub(super) const SCHEMA: &str = r#"
CREATE TABLE tab_todo_heads (
    tab_id TEXT PRIMARY KEY NOT NULL CHECK (length(tab_id) BETWEEN 1 AND 256),
    generation INTEGER NOT NULL CHECK (generation > 0)
) STRICT;
CREATE TABLE tab_todo_generations (
    tab_id TEXT NOT NULL,
    generation INTEGER NOT NULL CHECK (generation > 0),
    workspace_id TEXT NOT NULL CHECK (length(workspace_id) BETWEEN 1 AND 256),
    team_id TEXT CHECK (team_id IS NULL OR length(team_id) <= 256),
    status TEXT NOT NULL CHECK (status IN ('active', 'complete', 'cleared', 'closed', 'reset')),
    started_at INTEGER NOT NULL CHECK (started_at >= 0),
    revision INTEGER NOT NULL CHECK (revision > 0),
    PRIMARY KEY (tab_id, generation)
) STRICT;
CREATE TABLE tab_todo_items (
    tab_id TEXT NOT NULL,
    generation INTEGER NOT NULL CHECK (generation > 0),
    item_id TEXT NOT NULL CHECK (length(item_id) BETWEEN 1 AND 128),
    position INTEGER NOT NULL CHECK (position BETWEEN 0 AND 15),
    title TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 120),
    description TEXT CHECK (description IS NULL OR length(CAST(description AS BLOB)) <= 512),
    state TEXT NOT NULL CHECK (state IN ('open', 'in_progress', 'done')),
    revision INTEGER NOT NULL CHECK (revision > 0),
    deleted INTEGER NOT NULL CHECK (deleted IN (0, 1)),
    link_json TEXT CHECK (link_json IS NULL OR length(CAST(link_json AS BLOB)) <= 4096),
    PRIMARY KEY (tab_id, generation, item_id),
    FOREIGN KEY (tab_id, generation)
        REFERENCES tab_todo_generations(tab_id, generation)
) STRICT;
CREATE UNIQUE INDEX tab_todo_live_positions
    ON tab_todo_items(tab_id, generation, position) WHERE deleted = 0;
CREATE INDEX tab_todo_items_order
    ON tab_todo_items(tab_id, generation, deleted, position);
CREATE TABLE tab_todo_audit (
    event_id INTEGER PRIMARY KEY,
    tab_id TEXT NOT NULL CHECK (length(tab_id) BETWEEN 1 AND 256),
    generation INTEGER NOT NULL CHECK (generation > 0),
    item_id TEXT CHECK (item_id IS NULL OR length(item_id) <= 128),
    actor_tab_id TEXT CHECK (actor_tab_id IS NULL OR length(actor_tab_id) <= 256),
    action TEXT NOT NULL CHECK (action IN ('create', 'append', 'state_change', 'amend', 'clear_one', 'clear_all', 'reset', 'close', 'title_truncated')),
    item_revision INTEGER CHECK (item_revision IS NULL OR item_revision > 0),
    happened_at INTEGER NOT NULL CHECK (happened_at >= 0)
) STRICT;
CREATE INDEX tab_todo_audit_by_tab
    ON tab_todo_audit(tab_id, event_id);
"#;
