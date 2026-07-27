import { uid } from "../../lib/uid";
import { TitleLevel } from "../../lib/types";
import type {
  MultiplexerState,
  Tab,
  Tab,
  Team,
  Notification,
  TabStatus,
} from "../../lib/types";

export interface ExtendedState extends MultiplexerState {
  teams: Team[];
  notifications: Notification[];
  tabStatuses: Record<string, TabStatus>;
  toasts: Notification[];
  activeTabIds: Record<string, string>;
  /** Highest snapshot generation applied so far; guards against stale snapshots. */
  generation: number;
  /** Tab ids removed by an optimistic close, masked out of accepted snapshots
   *  until an accepted snapshot omits them (self-clear) or an explicit remove. */
  pendingCloses: string[];
}

export function makeTab(tab: Tab): Tab {
  return {
    id: uid(),
    description: tab.description ?? null,
    tab_id: tab.id,
  };
}

/** Keep client state aligned with the daemon's one-tab-per-tab invariant.
 * A stale restore snapshot must never make closed history look like live tabs. */
export function reconcileBackendWorkspaceTabs(
  workspaces: MultiplexerState["workspaces"],
  tabs: Record<string, Tab>,
): MultiplexerState["workspaces"] {
  const claimedTabIds = new Set<string>();
  const claimedTabIds = new Set<string>();
  return workspaces.map((workspace) => {
    const tabs = workspace.tabs.filter((tab) => {
      if (!tabs[tab.tab_id] || claimedTabIds.has(tab.id) || claimedTabIds.has(tab.tab_id)) {
        return false;
      }
      claimedTabIds.add(tab.id);
      claimedTabIds.add(tab.tab_id);
      return true;
    });
    const active_tab_id = tabs.some((tab) => tab.id === workspace.active_tab_id)
      ? workspace.active_tab_id
      : tabs[0]?.id ?? null;
    return { ...workspace, tabs, active_tab_id };
  });
}

export function displayTitleForTab(tab: Tab, tab?: Tab | null): string {
  const title = tab?.title?.trim();
  if (title) return title;
  // Fallback to tab_id prefix — never a bare "…" so ghost tabs are identifiable.
  const hex = tab.tab_id.slice(0, 8);
  return import.meta.env?.DEV ? `[tab:${hex}]` : hex;
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
