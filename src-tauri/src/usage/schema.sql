PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA synchronous = NORMAL;

CREATE TABLE IF NOT EXISTS usage_snapshots (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  provider TEXT NOT NULL,
  account_id TEXT,
  captured_at INTEGER NOT NULL,
  parsed_ok INTEGER NOT NULL,
  parse_error TEXT,
  raw_text TEXT,
  session_pct REAL,
  session_resets_at INTEGER,
  weekly_pct REAL,
  weekly_resets_at INTEGER,
  weekly_sonnet_pct REAL,
  weekly_sonnet_resets_at INTEGER,
  session_cost_usd REAL,
  session_api_duration_s INTEGER,
  session_wall_duration_s INTEGER,
  session_lines_added INTEGER,
  session_lines_removed INTEGER,
  session_tokens_input INTEGER,
  session_tokens_output INTEGER,
  session_tokens_cache_read INTEGER,
  session_tokens_cache_write INTEGER
);
CREATE INDEX IF NOT EXISTS idx_usage_provider_ts ON usage_snapshots(provider, captured_at DESC);
CREATE INDEX IF NOT EXISTS idx_usage_account_ts ON usage_snapshots(account_id, captured_at DESC);

DROP TABLE IF EXISTS usage_breakdown;
