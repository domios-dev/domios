PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
PRAGMA synchronous = NORMAL;

CREATE TABLE IF NOT EXISTS usage_snapshots (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  provider TEXT NOT NULL,
  account_id TEXT,
  captured_at INTEGER NOT NULL,
  parsed_ok INTEGER NOT NULL,
  billing_mode TEXT NOT NULL DEFAULT 'unknown',
  limits_reported INTEGER NOT NULL DEFAULT 0,
  session_pct REAL,
  session_resets_at INTEGER,
  weekly_pct REAL,
  weekly_resets_at INTEGER,
  spend_cents REAL,
  spend_limit_cents REAL,
  spend_remaining_cents REAL,
  spend_resets_at INTEGER,
  failure_reason TEXT,
  probe_command TEXT,
  raw_excerpt TEXT,
  debug_json TEXT
);
CREATE INDEX IF NOT EXISTS idx_usage_provider_ts ON usage_snapshots(provider, captured_at DESC);
CREATE INDEX IF NOT EXISTS idx_usage_account_ts ON usage_snapshots(account_id, captured_at DESC);

DROP TABLE IF EXISTS usage_breakdown;
