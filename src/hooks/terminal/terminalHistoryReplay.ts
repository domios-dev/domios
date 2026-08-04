const hydratedTerminals = new WeakSet<object>();
const activeTerminals = new WeakSet<object>();
let nextClaimId = 0;

export interface TerminalHistoryClaim {
  tabId: string;
  id: number;
  terminal: object;
}

export function claimInitialTerminalHistory(tabId: string, terminal: object): TerminalHistoryClaim | null {
  if (hydratedTerminals.has(terminal) || activeTerminals.has(terminal)) return null;
  const claim = { tabId, id: ++nextClaimId, terminal };
  activeTerminals.add(terminal);
  return claim;
}

export function completeInitialTerminalHistory(claim: TerminalHistoryClaim): void {
  if (!activeTerminals.has(claim.terminal)) return;
  activeTerminals.delete(claim.terminal);
  hydratedTerminals.add(claim.terminal);
}

export function abandonInitialTerminalHistory(claim: TerminalHistoryClaim): void {
  activeTerminals.delete(claim.terminal);
}
