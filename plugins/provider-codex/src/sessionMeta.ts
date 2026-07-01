declare const host: {
  fs: {
    readFile(path: string): string | null;
  };
};

export function readSessionMeta(path: string): { sessionId: string | null; cwd: string | null } | null {
  const contents = host.fs.readFile(path);
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
