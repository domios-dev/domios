import { ioMap, type HostIoSteps } from "@codeterm/plugin-sdk";
import { effectiveReadPath } from "../../provider-shared/readHome";

declare const host: import("@codeterm/plugin-sdk").Host;

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

export function* sessionFilePath(_cwd: string, sessionId: string): HostIoSteps<string | null> {
  // Codex records the rollout it is appending to on the thread row itself. It is a
  // real JSONL file and the usage parser reads it; the transcript did not move.
  const path = trimmedField((yield* threadRow(sessionId)), "rollout_path");
  return path && (yield* host.io.fs.fileExists(path)) ? path : null;
}

export function* sessionExists(_cwd: string, sessionId: string): HostIoSteps<boolean> {
  if (!sessionId) return false;
  if ((yield* threadRow(sessionId)) !== null) return true;
  const store = codexSessionStore();
  return store !== null && (yield* sessionIndexEntry(store.indexPath, sessionId)) !== null;
}

export function* detectSessionId(cwd: string, exclude?: string[] | string, maxAgeMs?: number): HostIoSteps<string | null> {
  if (!cwd || !(yield* host.io.fs.fileExists(cwd))) return null;
  const store = codexSessionStore();
  if (!store) return null;
  const entries = (yield* readSessionIndex(store.indexPath));
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
    const project = (yield* readThreadCwd(entry.id));
    if (project && host.path.equal(project, cwd)) matching.push(entry.id);
  }
  // The index has no cwd and can contain several live threads. Ambiguity is unknown, not recency.
  return matching.length === 1 ? matching[0] : null;
}

export function* enumerateSessions(): HostIoSteps<unknown[]> {
  const store = codexSessionStore();
  if (!store) return [];
  const entries = (yield* readSessionIndex(store.indexPath));
  return (yield* ioMap(entries, function*(entry) { return ({
    sessionId: entry.id,
    provider: "codex",
    projectPath: (yield* readThreadCwd(entry.id)),
    title: entry.thread_name || null,
    summary: null,
    filePath: store.indexPath,
    createdTs: null,
    modifiedTs: sessionTimestamp(entry.updated_at) > 0
      ? Math.floor(sessionTimestamp(entry.updated_at) / 1000)
      : null,
  }); }));
}

export function* findSession(sessionId: string): HostIoSteps<unknown | null> {
  // Whole row, so a Codex schema without the newer columns still resolves the thread.
  const row = (yield* queryThreadRow(sessionId, "SELECT * FROM threads WHERE id = ? LIMIT 1"));
  if (!row) return null;
  const store = codexSessionStore();
  return {
    sessionId,
    provider: "codex",
    projectPath: (yield* resolveThreadCwd(sessionId, row)),
    title: trimmedField(row, "name") || trimmedField(row, "title"),
    summary: null,
    filePath: store ? store.indexPath : ((yield* codexStateDbPath()) || ""),
    createdTs: unixSeconds(row.created_at),
    modifiedTs: unixSeconds(row.updated_at),
  };
}

export function* detectSessionModel(_cwd: string, sessionId: string): HostIoSteps<string | null> {
  return trimmedField((yield* threadRow(sessionId)), "model");
}

export function* detectSessionReasoningEffort(_cwd: string, sessionId: string): HostIoSteps<string | null> {
  const path = (yield* sessionFilePath(_cwd, sessionId));
  if (!path) return null;
  const contents = (yield* host.io.fs.readFileTail(path, 4 * 1024 * 1024));
  if (typeof contents !== "string") return null;
  const lines = contents.split("\n");
  for (let index = lines.length - 1; index >= 0; index--) {
    let entry: { type?: unknown; payload?: { effort?: unknown } };
    try { entry = JSON.parse(lines[index]); } catch (_) { continue; }
    if (entry.type !== "turn_context" || typeof entry.payload?.effort !== "string") continue;
    const effort = entry.payload.effort.trim();
    if (effort) return effort;
  }
  return null;
}

interface CodexThreadRow {
  model?: unknown;
  rollout_path?: unknown;
  cwd?: unknown;
  name?: unknown;
  title?: unknown;
  created_at?: unknown;
  updated_at?: unknown;
}

function* threadRow(sessionId: string): HostIoSteps<CodexThreadRow | null> {
  return (yield* queryThreadRow(sessionId, "SELECT model, rollout_path, cwd FROM threads WHERE id = ? LIMIT 1"));
}

function* queryThreadRow(sessionId: string, sql: string): HostIoSteps<CodexThreadRow | null> {
  if (!sessionId) return null;
  const db = (yield* codexStateDbPath());
  if (!db || !(yield* host.io.fs.fileExists(db))) return null;
  try {
    const rows = (yield* host.io.fs.sqliteQuery<CodexThreadRow>(db, sql, [sessionId]));
    return rows.length > 0 ? rows[0] : null;
  } catch (_) {
    return null;
  }
}

function unixSeconds(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) && value > 0 ? Math.floor(value) : null;
}

function trimmedField(
  row: CodexThreadRow | null,
  field: "model" | "rollout_path" | "cwd" | "name" | "title",
): string | null {
  const value = row ? row[field] : null;
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

export function* codexStateDbPath(): HostIoSteps<string | null> {
  const override = host.envGet(sessionStoreOverrides.state);
  if (override) return override;
  const sep = host.platform() === "windows" ? "\\" : "/";
  const root = codexSessionsRoot(sep);
  if (!root) return null;
  let entries: { name: string; isFile: boolean }[];
  try {
    entries = (yield* host.io.fs.readDir(root));
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
  return effectiveReadPath(host.envGet("CODEX_HOME"), home ? home + sep + ".codex" : null, "", sep);
}

function* readSessionIndex(path: string): HostIoSteps<CodexSessionIndexEntry[]> {
  if (!(yield* host.io.fs.fileExists(path))) return [];
  const contents = (yield* host.io.fs.readFile(path));
  return typeof contents === "string" ? foldSessionIndex(contents) : [];
}

function* sessionIndexEntry(path: string, sessionId: string): HostIoSteps<CodexSessionIndexEntry | null> {
  const entries = (yield* readSessionIndex(path));
  for (let i = 0; i < entries.length; i++) {
    if (entries[i].id === sessionId) return entries[i];
  }
  return null;
}

const unresolvedCwdDiagnostics = new Set<string>();

function* readThreadCwd(sessionId: string): HostIoSteps<string | null> {
  return (yield* resolveThreadCwd(sessionId, (yield* threadRow(sessionId))));
}

function* resolveThreadCwd(sessionId: string, row: CodexThreadRow | null): HostIoSteps<string | null> {
  if (!row) {
    logUnresolvedCwd(sessionId, "no_row");
    return null;
  }
  const cwd = trimmedField(row, "cwd");
  if (!cwd) {
    logUnresolvedCwd(sessionId, "no_cwd");
    return null;
  }
  if (!(yield* host.io.fs.fileExists(cwd))) {
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
