export interface ReturnTo {
  workspaceId: string;
  tabId: string;
}

export interface ReturnToRoute {
  returnTo?: ReturnTo;
}

export interface ReturnToChatContext {
  workspaces: readonly { id: string; tabs: readonly string[] }[];
  closePanels: () => void;
  setChatMode: (tabId: string) => void;
  setActiveTab: (workspaceId: string, tabId: string) => void;
  setFocusedTab: (tabId: string) => void;
}

export function activateReturnToChat(origin: ReturnTo, context: ReturnToChatContext): boolean {
  if (!context.workspaces.some((workspace) => workspace.id === origin.workspaceId && workspace.tabs.includes(origin.tabId))) return false;
  context.closePanels();
  context.setChatMode(origin.tabId);
  context.setActiveTab(origin.workspaceId, origin.tabId);
  context.setFocusedTab(origin.tabId);
  return true;
}

export function returnToOrigin(route: ReturnToRoute | null | undefined, activate: (origin: ReturnTo) => boolean): boolean {
  const origin = route?.returnTo;
  if (!origin) return false;
  delete route.returnTo;
  return activate(origin);
}
