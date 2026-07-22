declare const host: {
  fs: {
    readFileHead(path: string, maxBytes: number): string | null;
  };
};

// session_meta is the first JSONL record; a bounded head read avoids slurping a multi-hundred-MB rollout.
export const SESSION_META_HEAD_BYTES = 64 * 1024;
// Last turn_context lives near the end; a bounded tail read keeps model refresh off a whole-file scan.
export const SESSION_MODEL_TAIL_BYTES = 1024 * 1024;

export function readSessionMeta(path: string): { sessionId: string | null; cwd: string | null } | null {
  const contents = host.fs.readFileHead(path, SESSION_META_HEAD_BYTES);
  if (!contents) return null;
  const firstNl = contents.indexOf("\n");
  const line = firstNl === -1 ? contents : contents.substring(0, firstNl);
  let v: { type?: string; payload?: { id?: string; cwd?: string } };
  try { v = JSON.parse(line.trim()); } catch (_) { return null; }
  if (!v || v.type !== "session_meta") return null;
  const p = v.payload || {};
  return {
    sessionId: typeof p.id === "string" ? p.id : null,
    cwd: typeof p.cwd === "string" ? p.cwd : null,
  };
}
