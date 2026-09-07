import type { MultiplexerState } from "../../lib/types";

export interface TabGuard {
  tabId: string;
}

export type TabGuards = Record<string, TabGuard>;

// Mask the backend snapshot until it confirms or expires so an in-flight tab selection does not flash back.
export function applyPendingTabGuard(
  s: MultiplexerState,
  guards: TabGuards,
): MultiplexerState {
  let masked: MultiplexerState["workspaces"] | null = null;
  for (let i = 0; i < s.workspaces.length; i++) {
    const ws = s.workspaces[i];
    const guard = guards[ws.id];
    if (!guard) continue;
    if (ws.active_tab_id === guard.tabId) {
      delete guards[ws.id];
      continue;
    }
    // Target tab gone server-side: server wins, don't mask.
    if (!ws.tabs.includes(guard.tabId)) {
      delete guards[ws.id];
      continue;
    }
    if (!masked) masked = s.workspaces.slice();
    masked[i] = { ...ws, active_tab_id: guard.tabId };
  }
  if (!masked) return s;
  return { ...s, workspaces: masked };
}
