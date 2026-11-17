const GENERIC = "Sign-in failed. Try again.";
const NOTIFY = "Phone notification unavailable. Check your notification channels.";

function reasonText(error: unknown): string | undefined {
  if (typeof error === "string") return error;
  if (typeof error !== "object" || error === null) return undefined;
  const record = error as Record<string, unknown>;
  if (typeof record.body === "string") {
    try {
      const parsed = JSON.parse(record.body) as { error?: { message?: unknown }; message?: unknown };
      const message = parsed?.error?.message ?? parsed?.message;
      if (typeof message === "string") return message;
    } catch {
      return record.body;
    }
  }
  return typeof record.message === "string" && !("status" in record) ? record.message : undefined;
}

/** The daemon's sign-in refusal is already a safe, user-facing reason; transport noise falls back to the generic text. */
export function runtimeLoginErrorMessage(error: unknown, action?: string): string {
  if (action === "notify") return NOTIFY;
  const reason = reasonText(error)?.replace(/\s+/g, " ").trim();
  if (!reason || reason.startsWith("<") || reason.startsWith("{") || reason.length > 400) return GENERIC;
  return reason;
}
