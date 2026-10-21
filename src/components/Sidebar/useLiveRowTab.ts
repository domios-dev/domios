import { useCallback, useContext } from "react";
import { useSyncExternalStoreWithSelector } from "use-sync-external-store/shim/with-selector";
import type { Tab, TabAgentState } from "../../lib/types";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import type { ExtendedState } from "../../store/multiplexer/tree";

const detached = () => () => {};
const noState = () => null;

function sameAgentState(a: TabAgentState | null | undefined, b: TabAgentState | null | undefined): boolean {
  if (a === b) return true;
  if (!a || !b) return false;
  const keys = new Set([...Object.keys(a), ...Object.keys(b)] as (keyof TabAgentState)[]);
  for (const key of keys) if (a[key] !== b[key]) return false;
  return true;
}

/** The row's tab with its live agent state, which the sidebar leaves to each row while the tab keeps working. */
export function useLiveRowTab(tab: Tab): Tab {
  const store = useContext(MuxStateStoreContext);
  const select = useCallback(
    (state: ExtendedState | null) => state ? state.tabs[tab.id]?.agent_state : tab.agent_state,
    [tab],
  );
  const live = useSyncExternalStoreWithSelector(
    store?.subscribe ?? detached,
    store?.getState ?? noState,
    store?.getState ?? noState,
    select,
    sameAgentState,
  );
  return live === undefined || sameAgentState(live, tab.agent_state) ? tab : { ...tab, agent_state: live };
}
