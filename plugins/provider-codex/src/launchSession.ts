// Codex writes CodeTerm's own launch text into the rollout's early developer and
// user messages. A thread binds this tab only when that text carries this
// launch's marker; neither cwd nor recency decides.
import type { LaunchSessionDetection, LaunchSessionEvidence } from "@codeterm/plugin-sdk";
import { codexStateDbPath } from "./sessions";

const MARKER_PREFIX = "codeterm-launch-marker:";
const MIN_MARKER_LENGTH = 16;
const ROLLOUT_HEAD_BYTES = 262144;
const EARLY_RECORDS = 24;
const CANDIDATE_THREADS = 64;

interface ThreadRow {
  id?: unknown;
  rollout_path?: unknown;
}

interface RolloutEntry {
  type?: string;
  payload?: {
    id?: unknown;
    type?: string;
    role?: string;
    content?: Array<{ text?: unknown }>;
  };
}

export function withLaunchMarker(instructions: string, marker: string | undefined): string {
  if (!marker) return instructions;
  return instructions + "\n\n<!-- " + MARKER_PREFIX + " " + marker + " -->\n";
}

export function detectCodexLaunchSession(evidence: LaunchSessionEvidence): LaunchSessionDetection | null {
  const marker = evidence && typeof evidence.launchMarker === "string" ? evidence.launchMarker : "";
  if (marker.length < MIN_MARKER_LENGTH) return null;
  const db = codexStateDbPath();
  if (!db || !host.fs.fileExists(db)) return null;
  const since = Math.floor(Math.max(0, Number(evidence.launchedAtMs) || 0) / 1000);
  const rows = host.fs.sqliteQuery<ThreadRow>(
    db,
    "SELECT id, rollout_path FROM threads WHERE created_at >= ? ORDER BY created_at DESC LIMIT "
      + CANDIDATE_THREADS,
    [since],
  );
  const matches: string[] = [];
  for (let i = 0; i < rows.length; i++) {
    const id = typeof rows[i].id === "string" ? (rows[i].id as string) : "";
    const path = typeof rows[i].rollout_path === "string" ? (rows[i].rollout_path as string) : "";
    if (!id || !path || matches.indexOf(id) !== -1) continue;
    if (rolloutCarriesMarker(path, id, marker)) matches.push(id);
  }
  return matches.length === 1 ? { sessionId: matches[0], source: "launch_marker" } : null;
}

function rolloutCarriesMarker(path: string, id: string, marker: string): boolean {
  const head = host.fs.readFileHead(path, ROLLOUT_HEAD_BYTES);
  if (typeof head !== "string") return false;
  const lines = head.split("\n");
  let records = 0;
  for (let i = 0; i < lines.length && records < EARLY_RECORDS; i++) {
    if (!lines[i].trim()) continue;
    records++;
    let entry: RolloutEntry;
    try {
      entry = JSON.parse(lines[i]);
    } catch (_) {
      if (records === 1) return false;
      continue;
    }
    const payload = entry && entry.payload;
    if (records === 1) {
      if (entry.type !== "session_meta" || !payload || payload.id !== id) return false;
      continue;
    }
    if (entry.type !== "response_item" || !payload || payload.type !== "message") continue;
    if (payload.role !== "developer" && payload.role !== "user") continue;
    const content = Array.isArray(payload.content) ? payload.content : [];
    for (let j = 0; j < content.length; j++) {
      const text = content[j] && content[j].text;
      if (typeof text === "string" && text.indexOf(marker) !== -1) return true;
    }
  }
  return false;
}
