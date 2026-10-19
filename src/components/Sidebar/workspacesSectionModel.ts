export const SIDEBAR_WORKSPACES_SECTION_EXPANDED_KEY = "codeterm.sidebar.workspaces-section-expanded";

export function loadWorkspacesSectionExpanded(): boolean {
  try {
    return window.localStorage.getItem(SIDEBAR_WORKSPACES_SECTION_EXPANDED_KEY) !== "0";
  } catch {
    return true;
  }
}

export function persistWorkspacesSectionExpanded(expanded: boolean) {
  try {
    window.localStorage.setItem(SIDEBAR_WORKSPACES_SECTION_EXPANDED_KEY, expanded ? "1" : "0");
  } catch {}
}

export function workspaceSectionBodyVisible(isCompact: boolean, expanded: boolean): boolean {
  return isCompact || expanded;
}

export function toggleWorkspacesSectionExpanded(expanded: boolean): boolean {
  return !expanded;
}
