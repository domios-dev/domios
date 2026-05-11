PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;

CREATE TABLE IF NOT EXISTS app_logs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  ts INTEGER NOT NULL,
  level TEXT NOT NULL,
  source TEXT NOT NULL,
  message TEXT NOT NULL,
  context_json TEXT
);
CREATE INDEX IF NOT EXISTS idx_app_logs_ts ON app_logs(ts DESC);
CREATE INDEX IF NOT EXISTS idx_app_logs_level ON app_logs(level, ts DESC);
CREATE INDEX IF NOT EXISTS idx_app_logs_source ON app_logs(source, ts DESC);
