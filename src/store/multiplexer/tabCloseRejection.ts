import type { Notice } from "../../lib/notices";

export type TabCloseRejectionReason = "protected" | "reservation_lost" | "membership_locked" | "unknown";

export function tabCloseRejectionReason(error: unknown): TabCloseRejectionReason {
  const message = error instanceof Error ? error.message : String(error ?? "");
  if (/protected Tab/i.test(message)) return "protected";
  if (/Tab close reservation lost|Tab is closing/i.test(message)) return "reservation_lost";
  if (/Workspace is closing|Workspace close reservation lost/i.test(message)) return "membership_locked";
  return "unknown";
}

const REASON_DETAIL: Record<TabCloseRejectionReason, string> = {
  protected: "This tab is protected and cannot be closed.",
  reservation_lost: "Another close of this tab is already in progress.",
  membership_locked: "Its workspace is being changed; try again when that finishes.",
  unknown: "The close was refused.",
};

/// The toast for a refused close, so a restored row never looks like a silent resurrection.
export function tabCloseRejectionNotice(error: unknown, workspaceId: string, tabId: string): Notice {
  const reason = tabCloseRejectionReason(error);
  const raw = error instanceof Error ? error.message : String(error ?? "");
  return {
    kind: "toast",
    id: `tab-close-rejected:${tabId}`,
    severity: "Error",
    title: "Tab was not closed",
    detail: reason === "unknown" && raw ? `${REASON_DETAIL.unknown} ${raw}` : REASON_DETAIL[reason],
    tabId,
    workspaceId,
  };
}
