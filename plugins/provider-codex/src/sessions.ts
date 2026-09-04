import { readSessionMeta, SESSION_MODEL_TAIL_BYTES } from "./sessionMeta";

declare const host: {
  envGet(name: string): string | null;
  homeDir(): string | null;
  platform(): string;
  unixNowMs?: () => number;
  path: { equal(a: string, b: string): boolean };
  fs: {
    fileExists(path: string): boolean;
    walkDir(path: string, maxDepth?: number): { name: string; path: string; isFile: boolean; modifiedMs: number | null; createdMs: number | null }[];
    readFileTail(path: string, maxBytes: number): string | null;
  };
};

interface SessionEntry {
  type?: string;
  payload?: { model?: unknown };
}

const sessionPathCache: Record<string, string> = {};

export function sessionFilePath(_cwd: string, sessionId: string): string | null {
  const root = codexSessionsRoot();
  if (!root) return null;
  const remembered = sessionPathCache[sessionId];
  if (remembered && host.fs.fileExists(remembered)) return remembered;
  let hit: string | null = null;
  const files = host.fs.walkDir(root, 5);
  for (let i = 0; i < files.length; i++) {
    const f = files[i];
    if (!f.isFile || !endsWith(f.name, ".jsonl")) continue;
    const stem = f.name.substring(0, f.name.length - 6);
    const meta = readSessionMeta(f.path);
    const storedId = meta && meta.sessionId;
    const matched = storedId
      ? storedId === sessionId
      : stem === sessionId || extractSessionUuid(stem) === sessionId;
    if (matched) {
      hit = f.path;
      break;
    }
  }
  if (hit) sessionPathCache[sessionId] = hit;
  return hit;
}

export function detectSessionId(cwd: string, exclude?: string[] | string, maxAgeMs?: number): string | null {
  const root = codexSessionsRoot();
  if (!root || !host.fs.fileExists(root)) return null;
  const excluded = new Set(Array.isArray(exclude) ? exclude : (exclude ? [exclude] : []));
  const now = host.unixNowMs ? host.unixNowMs() : Date.now();
  const ageLimit = Number.isFinite(maxAgeMs) && (maxAgeMs as number) > 0 ? (maxAgeMs as number) : 300000;
  let best: { sid: string; modifiedMs: number } | null = null;
  const files = host.fs.walkDir(root, 5);
  for (let i = 0; i < files.length; i++) {
    const f = files[i];
    if (!f.isFile || !endsWith(f.name, ".jsonl")) continue;
    const modifiedMs = f.modifiedMs || 0;
    if (modifiedMs > 0 && now - modifiedMs > ageLimit) continue;
    const stem = f.name.substring(0, f.name.length - 6);
    const meta = readSessionMeta(f.path);
    const sid = (meta && meta.sessionId) || extractSessionUuid(stem);
    if (!sid || excluded.has(sid) || excluded.has(stem)) continue;
    const project = meta && meta.cwd;
    if (cwd && project && !host.path.equal(project, cwd)) continue;
    if (!best || modifiedMs > best.modifiedMs) best = { sid, modifiedMs };
  }
  return best ? best.sid : null;
}

export function enumerateSessions(): unknown[] {
  const root = codexSessionsRoot();
  if (!root || !host.fs.fileExists(root)) return [];
  const out: unknown[] = [];
  const files = host.fs.walkDir(root, 5);
  for (let i = 0; i < files.length; i++) {
    const f = files[i];
    if (!f.isFile || !endsWith(f.name, ".jsonl")) continue;
    const stem = f.name.substring(0, f.name.length - 6);
    const meta = readSessionMeta(f.path);
    const sid = (meta && meta.sessionId) || extractSessionUuid(stem);
    if (!sid) continue;
    out.push({
      sessionId: sid,
      provider: "codex",
      projectPath: meta && meta.cwd ? meta.cwd : null,
      title: null,
      summary: null,
      filePath: f.path,
      createdTs: f.createdMs ? Math.floor(f.createdMs / 1000) : null,
      modifiedTs: f.modifiedMs ? Math.floor(f.modifiedMs / 1000) : null,
    });
  }
  return out;
}

export function detectSessionModel(cwd: string, sessionId: string): string | null {
  const path = sessionFilePath(cwd, sessionId);
  if (!path) return null;
  const contents = host.fs.readFileTail(path, SESSION_MODEL_TAIL_BYTES);
  if (!contents) return null;
  const lines = contents.split("\n");
  for (let i = lines.length - 1; i >= 0; i--) {
    const line = lines[i];
    if (!line) continue;
    let v: SessionEntry;
    try { v = JSON.parse(line); } catch (_) { continue; }
    if (!v || v.type !== "turn_context") continue;
    const m = v.payload && v.payload.model;
    if (typeof m === "string" && m.trim()) return m.trim();
  }
  return null;
}

export function extractSessionUuid(stem: string): string | null {
  if (typeof stem !== "string") return null;
  const prefix = "rollout-";
  if (stem.indexOf(prefix) !== 0) return null;
  const rest = stem.substring(prefix.length);
  if (rest.length <= 20) return null;
  if (rest.charAt(19) !== "-") return null;
  return rest.substring(20);
}

function codexSessionsRoot(): string | null {
  const override = host.envGet("CODETERM_CODEX_SESSIONS_ROOT");
  if (override) return override;
  const home = host.homeDir();
  if (!home) return null;
  const sep = host.platform() === "windows" ? "\\" : "/";
  return home + sep + ".codex" + sep + "sessions";
}

function endsWith(s: string, suffix: string): boolean {
  return s.length >= suffix.length
    && s.substring(s.length - suffix.length) === suffix;
}
