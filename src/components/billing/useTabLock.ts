import { useCallback, useContext } from "react";
import { useSyncExternalStoreWithSelector } from "use-sync-external-store/shim/with-selector";
import type { Tab } from "../../lib/types";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { tabLockedView, type LockedView } from "./freeLimitModel";

const detached = () => () => {};
const noState = () => null;

export function useTabLock(tab: Pick<Tab, "id" | "locked" | "workspace_id" | "team_id"> | null | undefined): LockedView {
  const store = useContext(MuxStateStoreContext);
  const tabId = tab?.id;
  const workspaceId = tab?.workspace_id;
  const teamId = tab?.team_id;
  const select = useCallback(
    (state: ExtendedState | null) => {
      const workspaceLocked = state?.workspaces.some((workspace) => workspace.locked === true
        && (workspace.tabs.includes(tabId ?? "") || (workspaceId != null && workspace.id === workspaceId))) ?? false;
      if (workspaceLocked) return "workspaces";
      const liveTab = tabId ? state?.tabs[tabId] : undefined;
      const liveTeamId = liveTab ? liveTab.team_id : teamId;
      return state?.teams?.some((team) => team.locked === true && team.id === liveTeamId) ? "teams" : null;
    },
    [tabId, workspaceId, teamId],
  );
  const inheritedLock = useSyncExternalStoreWithSelector(
    store?.subscribe ?? detached,
    store?.getState ?? noState,
    store?.getState ?? noState,
    select,
  );
  return tabLockedView(tab, { locked: inheritedLock === "workspaces" }, { locked: inheritedLock === "teams" });
}
