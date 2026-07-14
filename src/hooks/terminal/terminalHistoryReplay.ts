const hydratedPaneIds = new Set<string>();
const activeClaims = new Map<string, number>();
let nextClaimId = 0;

export interface TerminalHistoryClaim {
  paneId: string;
  id: number;
}

export function claimInitialTerminalHistory(paneId: string): TerminalHistoryClaim | null {
  if (hydratedPaneIds.has(paneId) || activeClaims.has(paneId)) return null;
  const claim = { paneId, id: ++nextClaimId };
  activeClaims.set(paneId, claim.id);
  return claim;
}

export function completeInitialTerminalHistory(claim: TerminalHistoryClaim): void {
  if (activeClaims.get(claim.paneId) !== claim.id) return;
  activeClaims.delete(claim.paneId);
  hydratedPaneIds.add(claim.paneId);
}

export function abandonInitialTerminalHistory(claim: TerminalHistoryClaim): void {
  if (activeClaims.get(claim.paneId) === claim.id) activeClaims.delete(claim.paneId);
}
