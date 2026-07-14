const hydratedPaneIds = new Set<string>();

export function claimInitialTerminalHistory(paneId: string): boolean {
  if (hydratedPaneIds.has(paneId)) return false;
  hydratedPaneIds.add(paneId);
  return true;
}
