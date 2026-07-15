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

/** A UI-only "New session" picker tab: a synthetic tab + pane that live only
 *  in the client. Keyed by workspace id (at most one open picker per workspace)
 *  so the reducer can re-inject them after every backend snapshot without them
 *  ever reaching the backend, the session snapshot, or the web cache. */
export interface VirtualPickerTab {
  tab: Tab;
  pane: Pane;
}

export interface ExtendedState extends MultiplexerState {
  teams: Team[];
  notifications: Notification[];
  paneStatuses: Record<string, PaneStatus>;
  toasts: Notification[];
  activeTabIds: Record<string, string>;
  /** Highest snapshot generation applied so far; guards against stale snapshots. */
  generation: number;
  /** Frontend-only virtual picker tabs, keyed by workspace id. Re-injected into
   *  `workspaces`/`panes` on every SET_STATE; never persisted or sent upstream. */
  virtualTabs: Record<string, VirtualPickerTab>;
}

export function makeTab(pane: Pane): Tab {
  return {
    id: uid(),
    description: pane.description ?? null,
    pane_id: pane.id,
  };
}

/** Build a fresh virtual picker tab+pane for a workspace. The pane carries a
 *  real title ("New session") from birth — a virtual tab must never render as
 *  an empty/placeholder/"…" husk. */
export function makeVirtualPickerTab(cwd?: string | null): VirtualPickerTab {
  const paneId = `virtual-picker:${uid()}`;
  const pane: Pane = {
    id: paneId,
    pty_id: null,
    pane_type: "Terminal",
    title: "New session",
    title_level: TitleLevel.User,
    cwd: cwd ?? null,
    agent_type: null,
    virtual: true,
  };
  return {
    tab: { id: `virtual-tab:${uid()}`, pane_id: paneId, sort_order: Number.MAX_SAFE_INTEGER },
    pane,
  };
}

/** Re-inject open virtual picker tabs into a freshly-merged backend snapshot.
 *  Only injects into workspaces that still exist; a picker whose workspace
 *  vanished is silently dropped. Preserves the virtual tab as the workspace's
 *  active tab when it was active before, so a background snapshot never yanks
 *  the user off an open picker. */
export function injectVirtualTabs(
  virtualTabs: Record<string, VirtualPickerTab>,
  workspaces: MultiplexerState["workspaces"],
  panes: Record<string, Pane>,
  activeTabIds: Record<string, string>,
  prevActiveTabIds: Record<string, string>,
): {
  workspaces: MultiplexerState["workspaces"];
  panes: Record<string, Pane>;
  activeTabIds: Record<string, string>;
} {
  const entries = Object.entries(virtualTabs);
  if (entries.length === 0) return { workspaces, panes, activeTabIds };
  const nextPanes = { ...panes };
  const nextActive = { ...activeTabIds };
  const nextWorkspaces = workspaces.map((ws) => {
    const v = virtualTabs[ws.id];
    if (!v) return ws;
    nextPanes[v.pane.id] = v.pane;
    if (prevActiveTabIds[ws.id] === v.tab.id) nextActive[ws.id] = v.tab.id;
    return { ...ws, tabs: [...ws.tabs, v.tab] };
  });
  return { workspaces: nextWorkspaces, panes: nextPanes, activeTabIds: nextActive };
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
