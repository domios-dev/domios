import { uid } from "../../lib/uid";
import { TitleLevel } from "../../lib/types";
import type {
  MultiplexerState,
  Pane,
  Tab,
  Notification,
  PaneStatus,
} from "../../lib/types";

export interface ExtendedState extends MultiplexerState {
  notifications: Notification[];
  paneStatuses: Record<string, PaneStatus>;
  toasts: Notification[];
  activeTabIds: Record<string, string>;
  /** Highest snapshot generation applied so far; guards against stale snapshots. */
  generation: number;
}

export function makeTab(pane: Pane): Tab {
  return {
    id: uid(),
    description: pane.description ?? null,
    pane_id: pane.id,
  };
}

export function displayTitleForTab(tab: Tab, pane?: Pane | null): string {
  const title = pane?.title?.trim() || tab.title?.trim();
  return title || tab.pane_id.slice(0, 8);
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
