import type { Tab } from "./types";

/** Resolve interruption through the tab's provider declaration. */
export function interruptAgentArgs(tab: Pick<Tab, "id" | "pty_id" | "managed">): { ptyId: string; tabId?: string } | null {
  if (!tab.pty_id) return null;
  return { ptyId: tab.pty_id, tabId: tab.id };
}
