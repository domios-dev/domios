import type { Tab, Workspace } from "../../lib/types";

/** A terminal agent tab without a PTY is woken; one that owns a PTY already has its backend, so spawning again only reuses it. */
export function needsBackendPrewarm(tab: Pick<Tab, "agent_type" | "backing" | "pty_id"> | undefined): boolean {
  return Boolean(tab && tab.agent_type && tab.backing?.kind !== "chat_backend" && !tab.pty_id);
}

export function workspacePrewarmInput(
  workspace: Workspace | null | undefined,
  activeTabId: string | null,
  tabs: Readonly<Record<string, Pick<Tab, "protected">>>,
): { workspaceId: string | null; activeTabId: string | null; tabIds: string[] } {
  if (!workspace || workspace.cwd_missing) return { workspaceId: null, activeTabId: null, tabIds: [] };
  return {
    workspaceId: workspace.id,
    activeTabId,
    tabIds: workspace.tabs.filter(id => tabs[id] && !(workspace.protected === true && tabs[id].protected === true)),
  };
}
