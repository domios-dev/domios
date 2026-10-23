import { useCallback, useContext } from "react";
import { useSyncExternalStoreWithSelector } from "use-sync-external-store/shim/with-selector";
import type { Tab } from "../../lib/types";
import { tabLockFromState } from "./lockSurfaces";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import type { ExtendedState } from "../../store/multiplexer/tree";
import { tabLockedView, type LockedView } from "./freeLimitModel";

const detached = () => () => {};
const noState = () => null;

/** The tab's lock view follows its own, workspace and Team marks. */
export function useTabLock(tab: Pick<Tab, "id" | "locked" | "workspace_id" | "team_id"> | null | undefined): LockedView {
  const store = useContext(MuxStateStoreContext);
  const select = useCallback(
    (state: ExtendedState | null) => state ? tabLockFromState(state, tab) : tabLockedView(tab, null),
    [tab],
  );
  return useSyncExternalStoreWithSelector(
    store?.subscribe ?? detached,
    store?.getState ?? noState,
    store?.getState ?? noState,
    select,
    (a, b) => a.locked === b.locked && (!a.locked || (b.locked && a.limit === b.limit)),
  );
}
