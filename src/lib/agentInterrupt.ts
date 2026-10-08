import type { Tab } from "./types";

/** `interrupt_agent` arguments for a tab; a managed tab routes through its manager while that manager is open. */
export function interruptAgentArgs(tab: Pick<Tab, "id" | "pty_id" | "managed">): { ptyId: string; tabId?: string } | null {
  if (!tab.pty_id) return null;
  return { ptyId: tab.pty_id, ...(tab.managed && !tab.managed.manager_closed ? { tabId: tab.id } : {}) };
}
