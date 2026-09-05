/// Tauri rejects a command with a plain string; the web bridge throws an Error.
/// Either shape carries the daemon's structural reason, and hiding it behind a
/// generic "Try again" is what made a permanent refusal look transient.
export function refusalText(error: unknown, fallback: string): string {
  if (typeof error === "string" && error.trim()) return error.trim();
  if (error instanceof Error && error.message.trim()) return error.message.trim();
  return fallback;
}
