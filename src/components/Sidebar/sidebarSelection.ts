/** Sidebar workspace surface: exactly one of plan entry or session tab is visually active. */

export function isWorkspacePlanEntryActive(
  workspaceIsActiveVisual: boolean,
  workspaceCanvasActive: boolean,
): boolean {
  return workspaceIsActiveVisual && workspaceCanvasActive;
}

export function isWorkspaceTabActive(
  workspaceIsActiveVisual: boolean,
  workspaceCanvasActive: boolean,
  tabId: string,
  activeTabId: string | undefined,
): boolean {
  if (!workspaceIsActiveVisual || workspaceCanvasActive) return false;
  return tabId === activeTabId;
}
