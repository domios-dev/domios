import { uid } from "../../lib/uid";
import { TitleLevel } from "../../lib/types";
import type {
  MultiplexerState,
  Pane,
  Tab,
  Team,
  Notification,
  PaneStatus,
} from "../../lib/types";

export interface ExtendedState extends MultiplexerState {
  teams: Team[];
  notifications: Notification[];
  paneStatuses: Record<string, PaneStatus>;
  toasts: Notification[];
  activeTabIds: Record<string, string>;
  /** Highest snapshot generation applied so far; guards against stale snapshots. */
  generation: number;
  /** Pane ids removed by an optimistic close, masked out of accepted snapshots
   *  until an accepted snapshot omits them (self-clear) or an explicit remove. */
  pendingCloses: string[];
}

export function makeTab(pane: Pane): Tab {
  return {
    id: uid(),
    description: pane.description ?? null,
    pane_id: pane.id,
  };
}

/** Keep client state aligned with the daemon's one-pane-per-pane invariant.
 * A stale restore snapshot must never make closed history look like live tabs. */
export function reconcileBackendWorkspaceTabs(
  workspaces: MultiplexerState["workspaces"],
  panes: Record<string, Pane>,
): MultiplexerState["workspaces"] {
  const claimedTabIds = new Set<string>();
  const claimedPaneIds = new Set<string>();
  return workspaces.map((workspace) => {
    const tabs = workspace.tabs.filter((tab) => {
      if (!panes[tab.pane_id] || claimedTabIds.has(tab.id) || claimedPaneIds.has(tab.pane_id)) {
        return false;
      }
      claimedTabIds.add(tab.id);
      claimedPaneIds.add(tab.pane_id);
      return true;
    });
    const active_tab_id = tabs.some((tab) => tab.id === workspace.active_tab_id)
      ? workspace.active_tab_id
      : tabs[0]?.id ?? null;
    return { ...workspace, tabs, active_tab_id };
  });
}

export function displayTitleForTab(tab: Tab, pane?: Pane | null): string {
  const title = pane?.title?.trim();
  if (title) return title;
  // Fallback to pane_id prefix — never a bare "…" so ghost tabs are identifiable.
  const hex = tab.pane_id.slice(0, 8);
  return import.meta.env?.DEV ? `[pane:${hex}]` : hex;
}

/** Compare two TitleLevel values and return the higher-priority one. */
export function highestTitleLevel(a: TitleLevel, b: TitleLevel): TitleLevel {
  const levels = [TitleLevel.Default, TitleLevel.Auto, TitleLevel.Agent, TitleLevel.User];
  return levels.indexOf(a) >= levels.indexOf(b) ? a : b;
}

/** Shorten terminal title for display — extract process name from full path */
export function shortenTitle(title: string): string {
  const pathMatch = title.match(/[/\\]([^/\\]+?)(?:\.exe)?$/i);
  if (pathMatch) {
    return pathMatch[1];
  }
  if (title.length > 50) {
    return title.slice(0, 47) + "...";
  }
  return title;
}
