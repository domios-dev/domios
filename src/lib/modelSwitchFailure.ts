function record(value: unknown): Record<string, unknown> | undefined {
  return typeof value === "object" && value !== null ? value as Record<string, unknown> : undefined;
}

function payload(error: unknown): Record<string, unknown> | undefined {
  const source = record(error);
  const body = source?.body ?? error;
  if (typeof body !== "string") return record(body);
  try { return record(JSON.parse(body)); } catch { return undefined; }
}

export function modelSwitchFailure(error: unknown, runtimeTab: boolean): { message: string; restartAvailable: boolean } {
  const detail = payload(error);
  const message = typeof detail?.error === "string" ? detail.error
    : error instanceof Error ? error.message : String(error);
  const restartAvailable = !runtimeTab && detail?.reason !== "runtime_control_refused"
    && (detail?.reason ? detail.restart_fallback_available === true : true);
  return { message, restartAvailable };
}
