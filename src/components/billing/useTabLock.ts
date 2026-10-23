import { useCallback, useContext } from "react";
import { useSyncExternalStoreWithSelector } from "use-sync-external-store/shim/with-selector";
import type { Tab } from "../../lib/types";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { tabLockedView, type LockedView } from "./freeLimitModel";

const detached = () => () => {};
const noState = () => null;

/** The tab's lock view, following its workspace's live lock mark so a locked workspace locks its tabs. */
export function useTabLock(tab: Pick<Tab, "id" | "locked" | "workspace_id"> | null | undefined): LockedView {
  const store = useContext(MuxStateStoreContext);
  const tabId = tab?.id;
  const workspaceId = tab?.workspace_id;
  const select = useCallback(
    (state: ExtendedState | null) => state?.workspaces.some((workspace) => workspace.locked === true
      && (workspace.tabs.includes(tabId ?? "") || (workspaceId != null && workspace.id === workspaceId))) ?? false,
    [tabId, workspaceId],
  );
  const workspaceLocked = useSyncExternalStoreWithSelector(
    store?.subscribe ?? detached,
    store?.getState ?? noState,
    store?.getState ?? noState,
    select,
  );
  return tabLockedView(tab, { locked: workspaceLocked });
}
