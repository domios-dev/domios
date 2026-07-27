import { TitleLevel } from "../../lib/types";
import type {
  MultiplexerState,
  Notification,
  Tab,
  TabStatus,
  Team,
} from "../../lib/types";

export interface ExtendedState extends MultiplexerState {
  teams: Team[];
  notifications: Notification[];
  tabStatuses: Record<string, TabStatus>;
  toasts: Notification[];
  activeTabIds: Record<string, string>;
  generation: number;
  pendingCloses: string[];
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
  return import.meta.env?.DEV ? `[tab:${prefix}]` : prefix;
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
