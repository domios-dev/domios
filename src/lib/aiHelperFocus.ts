export function selectAiHelperTab(
  workspaceId: string,
  tabId: string,
  setActiveTab: (workspaceId: string, tabId: string) => void,
  setFocusedTab: (tabId: string) => void,
): void {
  setActiveTab(workspaceId, tabId);
  setFocusedTab(tabId);
}
