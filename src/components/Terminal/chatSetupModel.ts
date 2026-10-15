import { tabActivity } from "../../lib/tabActivity";
import type { Tab } from "../../lib/types";
import type { ChatBodyView } from "./chatBodyVisibility";

export function chatSetupView(tab: Partial<Tab> | null | undefined, chatSessionAttached = false): ChatBodyView {
  if (tab?.boot_stage === "spawn_failed") return { kind: "launch_failed", reason: tab.boot_failure_reason ?? null };
  if (chatSessionAttached) return { kind: "none" };
  const activity = tabActivity(tab);
  return activity.launching || activity.kind === "failed"
    ? { kind: "starting", label: activity.label }
    : { kind: "none" };
}

export function heldMessagesForLaunch(sends: readonly { dismissed?: boolean; status?: { state: string } }[]): number {
  return sends.filter(send => !send.dismissed && send.status?.state === "held").length;
}
