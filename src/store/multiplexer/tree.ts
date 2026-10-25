import { TitleLevel } from "../../lib/types";
import type {
  MultiplexerState,
  Notification,
  Tab,
  TabStatus,
  Team,
} from "../../lib/types";
import { DEV_BUILD } from "../../lib/devBuild";

export interface ExtendedState extends MultiplexerState {
  teams: Team[];
  notifications: Notification[];
  /** False until the stored history has answered once, so an empty list is not mistaken for "no notifications". */
  notificationsLoaded?: boolean;
  tabStatuses: Record<string, TabStatus>;
  toasts: Notification[];
  activeTabIds: Record<string, string>;
  generation: number;
  pendingCloses: string[];
  storageUnavailable?: boolean;
  muxResyncRequired?: boolean;
  /** Set by the first user workspace or tab selection; a later restore seed is then ignored. */
  selectionNavigated?: boolean;
  /** Counts restore seeds applied, so a context binding can tell a restore from a user change. */
  selectionSeeds?: number;
}

export function reconcileBackendWorkspaceTabs(
  workspaces: MultiplexerState["workspaces"],
  tabs: Record<string, Tab>,
): MultiplexerState["workspaces"] {
  const claimed = new Set<string>();
  return workspaces.map((workspace) => {
    const workspaceTabs = workspace.tabs.filter((tabId) => {
      if (!tabs[tabId] || claimed.has(tabId)) return false;
      claimed.add(tabId);
      return true;
    });
    const active_tab_id =
      workspace.active_tab_id && workspaceTabs.includes(workspace.active_tab_id)
        ? workspace.active_tab_id
        : workspaceTabs[0] ?? null;
    return { ...workspace, tabs: workspaceTabs, active_tab_id };
  });
}

export function displayTitleForTab(tab: Tab): string {
  const title = tab.title?.trim();
  if (title) return title;
  const prefix = tab.id.slice(0, 8);
  return DEV_BUILD ? `[tab:${prefix}]` : prefix;
}

export function highestTitleLevel(a: TitleLevel, b: TitleLevel): TitleLevel {
  const levels = [TitleLevel.Default, TitleLevel.Auto, TitleLevel.Agent, TitleLevel.User];
  return levels.indexOf(a) >= levels.indexOf(b) ? a : b;
}

export function shortenTitle(title: string): string {
  const pathMatch = title.match(/[/\\]([^/\\]+?)(?:\.exe)?$/i);
  if (pathMatch) return pathMatch[1];
  return title.length > 50 ? `${title.slice(0, 47)}...` : title;
}
