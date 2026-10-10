import { useCallback, useContext, useRef, type MutableRefObject } from "react";
import { MuxStateStoreContext } from "../../../store/multiplexer/context";
import { useSidebarPlansOptional } from "../../Canvas/PlansProvider";
import type { Plan } from "../../Canvas/types";
import { expandDsTasksMention, type DsTasksExpansion } from "./dsTasksModel";

/** Renders nothing: it alone re-renders on plan events and leaves the latest plans in a ref. */
function PlansSink({ sink }: { sink: MutableRefObject<readonly Plan[]> }) {
  sink.current = useSidebarPlansOptional().plans;
  return null;
}

export function useDsTasksExpander(tabId: string) {
  const plansRef = useRef<readonly Plan[]>([]);
  const muxStore = useContext(MuxStateStoreContext);
  const expand = useCallback((text: string): DsTasksExpansion => {
    const state = muxStore?.getState();
    const tab = state?.tabs[tabId];
    return expandDsTasksMention(text, plansRef.current, {
      teamId: tab?.team_id ?? null,
      workspaceId: tab?.workspace_id ?? state?.active_workspace_id ?? null,
      teams: state?.teams ?? [],
      workspaces: state?.workspaces ?? [],
    });
  }, [muxStore, tabId]);
  return { expand, plansSink: <PlansSink sink={plansRef} /> };
}
