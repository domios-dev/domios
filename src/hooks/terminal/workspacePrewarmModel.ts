import type { Tab, Workspace } from "../../lib/types";

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
