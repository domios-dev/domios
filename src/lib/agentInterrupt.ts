import type { TabActivity } from "./tabActivity";
import type { Tab } from "./types";

type InterruptTarget = Pick<Tab, "id" | "pty_id" | "managed"> & Partial<Pick<Tab, "backing">>;

/** Resolve interruption through the tab's provider declaration; the tab stop route owns PTY and runtime turns alike. */
export function interruptAgentArgs(tab: InterruptTarget): { ptyId?: string; tabId: string } | null {
  if (tab.pty_id) return { ptyId: tab.pty_id, tabId: tab.id };
  if (tab.backing?.kind === "chat_backend") return { tabId: tab.id };
  return null;
}

/** Stop renders whenever the tab's agent is working and the tab has an interrupt route. */
export function showStopControl(activity: Pick<TabActivity, "canStop">, tab: InterruptTarget): boolean {
  return activity.canStop && interruptAgentArgs(tab) !== null;
}
