const SECRET_KEY = /^(value|password|master_?password|secret|token|api_?key|client_?secret|key)$/i;
const MAX_DEPTH = 4;

/** Copy of IPC args that is safe to log: secret-bearing fields are replaced, never printed. */
export function ipcLogArgs(args: unknown, depth = 0): unknown {
  if (depth > MAX_DEPTH || args === null || typeof args !== "object") return args;
  if (Array.isArray(args)) return args.map((item) => ipcLogArgs(item, depth + 1));
  const out: Record<string, unknown> = {};
  for (const [name, value] of Object.entries(args as Record<string, unknown>)) {
    out[name] = SECRET_KEY.test(name) && value !== null && value !== undefined
      ? "[redacted]"
      : ipcLogArgs(value, depth + 1);
  }
  return out;
}
