PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA synchronous = NORMAL;

CREATE TABLE IF NOT EXISTS usage_snapshots (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  provider TEXT NOT NULL,
  account_id TEXT,
  captured_at INTEGER NOT NULL,
  parsed_ok INTEGER NOT NULL,
  session_pct REAL,
  session_resets_at INTEGER,
  weekly_pct REAL,
  weekly_resets_at INTEGER
);
CREATE INDEX IF NOT EXISTS idx_usage_provider_ts ON usage_snapshots(provider, captured_at DESC);
CREATE INDEX IF NOT EXISTS idx_usage_account_ts ON usage_snapshots(account_id, captured_at DESC);

DROP TABLE IF EXISTS usage_breakdown;
