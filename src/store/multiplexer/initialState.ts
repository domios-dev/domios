import { loadCachedStateSync } from "../../lib/stateCache";
import { IS_WEB } from "../../lib/webApi";
import { loadClientPageSelection } from "../../lib/clientPageSelection";
import { initialPageSelection } from "./pageSelection";
import type { ExtendedState } from "./tree";

export const HYDRATED_GENERATION = 0.5;

export function buildInitialState(): ExtendedState {
  const empty: ExtendedState = {
    workspaces: [],
    tabs: {},
    teams: [],
    active_workspace_id: null,
    focused_tab_id: null,
    notifications: [],
    tabStatuses: {},
    toasts: [],
    activeTabIds: {},
    generation: 0,
    pendingCloses: [],
    storageUnavailable: false,
  };
  if (!IS_WEB) return empty;
  const cached = loadCachedStateSync();
  if (!cached) return empty;
  return {
    ...empty,
    workspaces: cached.workspaces,
    tabs: cached.tabs,
    teams: cached.teams ?? [],
    ...initialPageSelection(cached, loadClientPageSelection()),
    recently_closed: cached.recently_closed,
    closed_structures: cached.closed_structures, tabStatuses: cached.tab_statuses ?? {},
    generation: HYDRATED_GENERATION,
  };
}
