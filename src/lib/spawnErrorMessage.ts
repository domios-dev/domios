function record(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null ? value as Record<string, unknown> : null;
}

function errorBody(error: unknown): unknown {
  const body = record(error)?.body;
  const source = typeof body === "string" ? body
    : typeof error === "string" ? error : record(error)?.message;
  if (typeof source !== "string") return error;
  const start = source.indexOf("{");
  try { return JSON.parse(start < 0 ? source : source.slice(start)); } catch { return typeof body === "string" ? body : error; }
}

export function spawnErrorMessage(error: unknown): string {
  const parsed = errorBody(error);
  const envelope = record(parsed);
  const detail = record(envelope?.error);
  if (typeof detail?.message === "string" && detail.message.trim()) return detail.message;
  const message = envelope?.message
    ?? (typeof parsed === "string" ? parsed : undefined);
  if (typeof message === "string" && message.trim()
    && !message.includes("{") && !message.trim().startsWith("<")) return message;
  return "The session could not start. Check the workspace folder and try again.";
}

export function isWorkspaceCwdRefusal(error: unknown): boolean {
  return record(record(errorBody(error))?.error)?.code === "workspace_cwd_missing";
}
