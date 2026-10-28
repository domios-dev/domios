import type { NotificationAction, NotificationPromptIdentity } from "./types";
import { openNeighbor, openNeighborPage, type NeighborFocus } from "../components/Neighbors/neighborFocus";

function adoptPromptIdentity(raw: unknown): NotificationPromptIdentity | null {
  if (typeof raw !== "object" || raw === null) return null;
  const candidate = raw as { kind?: unknown; options?: unknown };
  const kinds = ["provider", "directory_trust", "structured", "manual_update"];
  if (!kinds.includes(String(candidate.kind)) || !Array.isArray(candidate.options)) return null;
  const options = candidate.options.flatMap((option) => {
    if (typeof option !== "object" || option === null) return [];
    const value = option as { label?: unknown; value?: unknown };
    return typeof value.label === "string" && typeof value.value === "string"
      ? [{ label: value.label, value: value.value }]
      : [];
  });
  if (options.length !== candidate.options.length || options.length < 2) return null;
  return { kind: candidate.kind as NotificationPromptIdentity["kind"], options };
}

/** Adopt only user-facing actions; legacy command actions are dropped. */
function adoptStoredAction(raw: unknown): NotificationAction | null {
  if (typeof raw !== "object" || raw === null) return null;
  const candidate = raw as { type?: unknown; label?: unknown; value?: unknown; identity?: unknown };
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
    case "respond_prompt_option": {
      const identity = adoptPromptIdentity(candidate.identity);
      return typeof candidate.value === "string" && identity
        ? { type: "respond_prompt_option", label: candidate.label, value: candidate.value, identity }
        : null;
    }
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
  if (action.type === "url" && action.value) {
    if (!handleNeighborLink(action.value)) window.open(action.value, "_blank");
  }
}

export function neighborNotificationTarget(value: string): NeighborFocus | null {
  const match = /^domios:\/\/neighbor\/([a-f0-9]{64})(?:\/(permissions|concierge))?$/.exec(value);
  if (!match || match[0] !== value) return null;
  const section = match[2];
  return { id: match[1], section: section === "permissions" || section === "concierge" ? section : null };
}

export function handleNeighborLink(value: string): boolean {
  if (!value.startsWith("domios://neighbor/")) return false;
  const target = neighborNotificationTarget(value);
  if (target?.section) openNeighborPage(target.id, target.section);
  else if (target) openNeighbor(target.id);
  return true;
}
