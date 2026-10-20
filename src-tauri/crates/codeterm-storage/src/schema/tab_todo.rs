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

/// Twelve-item bound, blocked/skipped states, attempts, lifetime counters and
/// the bounded per-item event log; existing items are kept as locked rows.
pub(super) const LIFECYCLE_SCHEMA: &str = r#"
CREATE TABLE tab_todo_items_lifecycle (
    tab_id TEXT NOT NULL,
    generation INTEGER NOT NULL CHECK (generation > 0),
    item_id TEXT NOT NULL CHECK (length(item_id) BETWEEN 1 AND 128),
    position INTEGER NOT NULL CHECK (position BETWEEN 0 AND 11),
    title TEXT NOT NULL CHECK (length(title) BETWEEN 1 AND 120),
    description TEXT CHECK (description IS NULL OR length(CAST(description AS BLOB)) <= 512),
    state TEXT NOT NULL CHECK (state IN ('open', 'in_progress', 'done', 'blocked', 'skipped')),
    revision INTEGER NOT NULL CHECK (revision > 0),
    deleted INTEGER NOT NULL CHECK (deleted IN (0, 1)),
    link_json TEXT CHECK (link_json IS NULL OR length(CAST(link_json AS BLOB)) <= 4096),
    locked INTEGER NOT NULL CHECK (locked IN (0, 1)),
    reason TEXT CHECK (reason IS NULL OR length(CAST(reason AS BLOB)) BETWEEN 1 AND 512),
    attempt INTEGER NOT NULL CHECK (attempt >= 0),
    attempt_closed INTEGER NOT NULL CHECK (attempt_closed IN (0, 1)),
    started_count INTEGER NOT NULL CHECK (started_count >= 0),
    completed_count INTEGER NOT NULL CHECK (completed_count >= 0),
    reopened_count INTEGER NOT NULL CHECK (reopened_count >= 0),
    blocked_count INTEGER NOT NULL CHECK (blocked_count >= 0),
    skipped_count INTEGER NOT NULL CHECK (skipped_count >= 0),
    folded_events INTEGER NOT NULL CHECK (folded_events >= 0),
    CHECK (deleted = 0 OR locked = 0),
    CHECK (state NOT IN ('blocked', 'skipped') OR reason IS NOT NULL),
    PRIMARY KEY (tab_id, generation, item_id),
    FOREIGN KEY (tab_id, generation)
        REFERENCES tab_todo_generations(tab_id, generation)
) STRICT;
INSERT INTO tab_todo_items_lifecycle (
    tab_id, generation, item_id, position, title, description, state, revision, deleted,
    link_json, locked, reason, attempt, attempt_closed, started_count, completed_count,
    reopened_count, blocked_count, skipped_count, folded_events
)
SELECT tab_id, generation, item_id, position, title, description, state, revision, deleted,
    link_json, 1 - deleted, NULL,
    CASE WHEN state = 'open' THEN 0 ELSE 1 END,
    CASE WHEN state = 'done' THEN 1 ELSE 0 END,
    CASE WHEN state = 'open' THEN 0 ELSE 1 END,
    CASE WHEN state = 'done' THEN 1 ELSE 0 END,
    0, 0, 0, 0
FROM tab_todo_items
WHERE position <= 11;
DROP TABLE tab_todo_items;
ALTER TABLE tab_todo_items_lifecycle RENAME TO tab_todo_items;
CREATE UNIQUE INDEX tab_todo_live_positions
    ON tab_todo_items(tab_id, generation, position) WHERE deleted = 0;
CREATE INDEX tab_todo_items_order
    ON tab_todo_items(tab_id, generation, deleted, position);
CREATE TABLE tab_todo_item_events (
    event_id INTEGER PRIMARY KEY,
    tab_id TEXT NOT NULL CHECK (length(tab_id) BETWEEN 1 AND 256),
    generation INTEGER NOT NULL CHECK (generation > 0),
    item_id TEXT NOT NULL CHECK (length(item_id) BETWEEN 1 AND 128),
    kind TEXT NOT NULL CHECK (kind IN ('started', 'completed', 'reopened', 'blocked', 'skipped')),
    attempt INTEGER NOT NULL CHECK (attempt >= 0),
    actor_tab_id TEXT CHECK (actor_tab_id IS NULL OR length(actor_tab_id) <= 256),
    reason TEXT CHECK (reason IS NULL OR length(CAST(reason AS BLOB)) BETWEEN 1 AND 512),
    happened_at INTEGER NOT NULL CHECK (happened_at >= 0)
) STRICT;
CREATE INDEX tab_todo_item_events_by_item
    ON tab_todo_item_events(tab_id, generation, item_id, event_id);
CREATE TABLE tab_todo_audit_lifecycle (
    event_id INTEGER PRIMARY KEY,
    tab_id TEXT NOT NULL CHECK (length(tab_id) BETWEEN 1 AND 256),
    generation INTEGER NOT NULL CHECK (generation > 0),
    item_id TEXT CHECK (item_id IS NULL OR length(item_id) <= 128),
    actor_tab_id TEXT CHECK (actor_tab_id IS NULL OR length(actor_tab_id) <= 256),
    action TEXT NOT NULL CHECK (action IN ('create', 'append', 'state_change', 'amend', 'clear_one', 'clear_all', 'reset', 'close', 'title_truncated', 'reopen', 'carry_over')),
    item_revision INTEGER CHECK (item_revision IS NULL OR item_revision > 0),
    happened_at INTEGER NOT NULL CHECK (happened_at >= 0)
) STRICT;
INSERT INTO tab_todo_audit_lifecycle SELECT * FROM tab_todo_audit;
DROP TABLE tab_todo_audit;
ALTER TABLE tab_todo_audit_lifecycle RENAME TO tab_todo_audit;
CREATE INDEX tab_todo_audit_by_tab
    ON tab_todo_audit(tab_id, event_id);
"#;

#[cfg(test)]
mod tests;
