import type { NotificationAction } from "./types";

/** Adopt only user-facing actions; legacy command actions are dropped. */
function adoptStoredAction(raw: unknown): NotificationAction | null {
  if (typeof raw !== "object" || raw === null) return null;
  const candidate = raw as { type?: unknown; label?: unknown; value?: unknown };
  if (typeof candidate.label !== "string") return null;
  switch (candidate.type) {
    case "url":
      return typeof candidate.value === "string"
        ? { type: "url", label: candidate.label, value: candidate.value }
        : null;
    case "dismiss":
      return { type: "dismiss", label: candidate.label };
    case "answer":
      return { type: "answer", label: candidate.label };
    default:
      return null;
  }
}

export function adoptStoredActions(raw: unknown): NotificationAction[] | undefined {
  if (raw === undefined || raw === null) return undefined;
  if (!Array.isArray(raw)) return undefined;
  return raw.flatMap((candidate) => {
    const action = adoptStoredAction(candidate);
    return action ? [action] : [];
  });
}

export async function runNotificationAction(action: NotificationAction): Promise<void> {
  if (action.type === "url" && action.value) window.open(action.value, "_blank");
}
