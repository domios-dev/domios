declare const host: {
  envGet(name: string): string | null;
  homeDir(): string | null;
  platform(): string;
  unixNowMs?: () => number;
  path: { equal(a: string, b: string): boolean };
  log(level: "trace" | "debug" | "info" | "warn" | "error", msg: string): void;
  fs: {
    fileExists(path: string): boolean;
    readFile(path: string): string | null;
    readDir(path: string): { name: string; isFile: boolean }[];
    sqliteQuery<T>(db: string, sql: string, params: unknown[]): T[];
  };
};

export interface CodexSessionIndexEntry {
  id: string;
  thread_name: string;
  updated_at: string;
}

interface CodexSessionStore {
  indexPath: string;
}

// Host-provided paths stay on the allowlist because the confinement heuristic rejects names containing `SESSION`.
const sessionStoreOverrides = {
  root: "CODETERM_CODEX_SESSIONS_ROOT",
  index: "CODETERM_CODEX_SESSION_INDEX_PATH",
  state: "CODETERM_CODEX_STATE_DB_PATH",
};

// Codex version-suffixes its state database and bumps the number; match the shape
// so a release that moves to the next one does not silently stop answering.
const STATE_DB_NAME = /^state_(\d+)\.sqlite$/;

export function foldSessionIndex(contents: string): CodexSessionIndexEntry[] {
  const folded = new Map<string, CodexSessionIndexEntry>();
  const lines = String(contents || "").split("\n");
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i].trim();
    if (!line) continue;
    let entry: Partial<CodexSessionIndexEntry>;
    try { entry = JSON.parse(line); } catch (_) { continue; }
    if (!entry || typeof entry.id !== "string" || !entry.id.trim()) continue;
    if (typeof entry.thread_name !== "string" || typeof entry.updated_at !== "string") continue;
    const current = folded.get(entry.id);
    if (!current || sessionTimestamp(entry.updated_at) > sessionTimestamp(current.updated_at)) {
      folded.set(entry.id, {
        id: entry.id,
        thread_name: entry.thread_name,
        updated_at: entry.updated_at,
      });
    }
  }
  return Array.from(folded.values());
}

export function sessionFilePath(_cwd: string, sessionId: string): string | null {
  // Codex records the rollout it is appending to on the thread row itself. It is a
  // real JSONL file and the usage parser reads it; the transcript did not move.
  const path = trimmedField(threadRow(sessionId), "rollout_path");
  return path && host.fs.fileExists(path) ? path : null;
}

export function sessionExists(_cwd: string, sessionId: string): boolean {
  if (!sessionId) return false;
  const store = codexSessionStore();
  return store !== null && sessionIndexEntry(store.indexPath, sessionId) !== null;
}

export function detectSessionId(cwd: string, exclude?: string[] | string, maxAgeMs?: number): string | null {
  if (!cwd || !host.fs.fileExists(cwd)) return null;
  const store = codexSessionStore();
  if (!store) return null;
  const entries = readSessionIndex(store.indexPath);
  if (entries.length === 0) return null;
  const excluded = new Set(Array.isArray(exclude) ? exclude : (exclude ? [exclude] : []));
  const now = host.unixNowMs ? host.unixNowMs() : Date.now();
  const ageLimit = Number.isFinite(maxAgeMs) && (maxAgeMs as number) > 0 ? (maxAgeMs as number) : 300000;
  const matching: string[] = [];
  for (let i = 0; i < entries.length; i++) {
    const entry = entries[i];
    if (excluded.has(entry.id)) continue;
    const updatedMs = sessionTimestamp(entry.updated_at);
    if (updatedMs > 0 && now - updatedMs > ageLimit) continue;
    const project = readThreadCwd(entry.id);
    if (project && host.path.equal(project, cwd)) matching.push(entry.id);
  }
  // The index has no cwd and can contain several live threads. Ambiguity is unknown, not recency.
  return matching.length === 1 ? matching[0] : null;
}

export function enumerateSessions(): unknown[] {
  const store = codexSessionStore();
  if (!store) return [];
  const entries = readSessionIndex(store.indexPath);
  return entries.map((entry) => ({
    sessionId: entry.id,
    provider: "codex",
    projectPath: readThreadCwd(entry.id),
    title: entry.thread_name || null,
    summary: null,
    filePath: store.indexPath,
    createdTs: null,
    modifiedTs: sessionTimestamp(entry.updated_at) > 0
      ? Math.floor(sessionTimestamp(entry.updated_at) / 1000)
      : null,
  }));
}

export function detectSessionModel(_cwd: string, sessionId: string): string | null {
  return trimmedField(threadRow(sessionId), "model");
}

interface CodexThreadRow {
  model?: unknown;
  rollout_path?: unknown;
  cwd?: unknown;
}

function threadRow(sessionId: string): CodexThreadRow | null {
  if (!sessionId) return null;
  const db = codexStateDbPath();
  if (!db || !host.fs.fileExists(db)) return null;
  try {
    const rows = host.fs.sqliteQuery<CodexThreadRow>(
      db,
      "SELECT model, rollout_path, cwd FROM threads WHERE id = ? LIMIT 1",
      [sessionId],
    );
    return rows.length > 0 ? rows[0] : null;
  } catch (_) {
    return null;
  }
}

function trimmedField(
  row: CodexThreadRow | null,
  field: "model" | "rollout_path" | "cwd",
): string | null {
  const value = row ? row[field] : null;
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function codexStateDbPath(): string | null {
  const override = host.envGet(sessionStoreOverrides.state);
  if (override) return override;
  const sep = host.platform() === "windows" ? "\\" : "/";
  const root = codexSessionsRoot(sep);
  if (!root) return null;
  let entries: { name: string; isFile: boolean }[];
  try {
    entries = host.fs.readDir(root);
  } catch (_) {
    return null;
  }
  let best: { name: string; version: number } | null = null;
  for (let i = 0; i < entries.length; i++) {
    const entry = entries[i];
    if (!entry || !entry.isFile) continue;
    const match = STATE_DB_NAME.exec(entry.name);
    if (!match) continue;
    const version = parseInt(match[1], 10);
    if (!best || version > best.version) best = { name: entry.name, version };
  }
  return best ? root + sep + best.name : null;
}

export function extractSessionUuid(stem: string): string | null {
  if (typeof stem !== "string") return null;
  const prefix = "rollout-";
  if (stem.indexOf(prefix) !== 0) return null;
  const rest = stem.substring(prefix.length);
  if (rest.length <= 20 || rest.charAt(19) !== "-") return null;
  return rest.substring(20);
}

function codexSessionStore(): CodexSessionStore | null {
  const indexOverride = host.envGet(sessionStoreOverrides.index);
  if (indexOverride) return { indexPath: indexOverride };
  const sep = host.platform() === "windows" ? "\\" : "/";
  const root = codexSessionsRoot(sep);
  if (!root) return null;
  return {
    indexPath: indexOverride || root + sep + "session_index.jsonl",
  };
}

function trimTrailingSeparators(path: string): string {
  let end = path.length;
  while (end > 1 && (path.charAt(end - 1) === "/" || path.charAt(end - 1) === "\\")) end--;
  return path.substring(0, end);
}

function codexSessionsRoot(sep: string): string | null {
  const override = host.envGet(sessionStoreOverrides.root);
  if (override) return trimTrailingSeparators(override);
  const home = host.homeDir();
  return home ? home + sep + ".codex" : null;
}

function readSessionIndex(path: string): CodexSessionIndexEntry[] {
  if (!host.fs.fileExists(path)) return [];
  const contents = host.fs.readFile(path);
  return typeof contents === "string" ? foldSessionIndex(contents) : [];
}

function sessionIndexEntry(path: string, sessionId: string): CodexSessionIndexEntry | null {
  const entries = readSessionIndex(path);
  for (let i = 0; i < entries.length; i++) {
    if (entries[i].id === sessionId) return entries[i];
  }
  return null;
}

const unresolvedCwdDiagnostics = new Set<string>();

function readThreadCwd(sessionId: string): string | null {
  const row = threadRow(sessionId);
  if (!row) {
    logUnresolvedCwd(sessionId, "no_row");
    return null;
  }
  const cwd = trimmedField(row, "cwd");
  if (!cwd) {
    logUnresolvedCwd(sessionId, "no_cwd");
    return null;
  }
  if (!host.fs.fileExists(cwd)) {
    logUnresolvedCwd(sessionId, "cwd_absent");
    return null;
  }
  return cwd;
}

function logUnresolvedCwd(sessionId: string, cause: "no_row" | "no_cwd" | "cwd_absent"): void {
  const key = sessionId + "\u0000" + cause;
  if (unresolvedCwdDiagnostics.has(key)) return;
  unresolvedCwdDiagnostics.add(key);
  host.log("warn", JSON.stringify({
    provider: "codex",
    event: "session_cwd_unresolved",
    sessionId,
    cause,
  }));
}

function sessionTimestamp(value: string): number {
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : 0;
}
