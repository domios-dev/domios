import type { PlanView as Plan } from "../../Canvas/planProjection";
import { memo, useCallback, useContext, useRef, type MutableRefObject } from "react";
import { MuxStateStoreContext } from "../../../store/multiplexer/context";
import { assertWebRequestContext, webRequestContext } from "../../../lib/webApi";
import { fetchRequestedPlanTaskMetadata, fetchPlanProjection } from "../../Canvas/plansApi";
import { usePlansApiOptional, useSidebarPlansOptional } from "../../Canvas/PlansProvider";
import { expandDsTasksMention, parseDsTasksInputs, resolveDsTasksPlan, type DsTasksExpansion } from "./dsTasksModel";

/** Renders nothing: it alone re-renders on plan events and leaves the latest plans in a ref. */
const PlansSink = memo(function PlansSink({ sink }: { sink: MutableRefObject<readonly Plan[]> }) {
  sink.current = useSidebarPlansOptional().plans;
  return null;
});

export function useDsTasksExpander(tabId: string) {
  const plansRef = useRef<readonly Plan[]>([]);
  const muxStore = useContext(MuxStateStoreContext);
  const api = usePlansApiOptional();
  const expand = useCallback(async (text: string): Promise<DsTasksExpansion> => {
    const state = muxStore?.getState();
    const tab = state?.tabs[tabId];
    const scope = {
      teamId: tab?.team_id ?? null,
      workspaceId: tab?.workspace_id ?? null,
      teams: state?.teams ?? [],
      workspaces: state?.workspaces ?? [],
    };
    const inputs = parseDsTasksInputs(text);
    if (!api || !inputs.length) return expandDsTasksMention(text, plansRef.current, scope);
    const generation = webRequestContext();
    const plans = new Map(plansRef.current.map((plan) => [plan.id, { ...plan, tasks: [] as Plan["tasks"] }]));
    const revisions = new Map<string,string>();
    for (const input of inputs) {
      assertWebRequestContext(generation);
      let plan = resolveDsTasksPlan([...plans.values()],input.args.planId,scope);
      if (!plan && input.args.planId) {
        plan = await fetchPlanProjection(api,input.args.planId,"summary");
        plans.set(plan.id,plan);
      }
      if (!plan) return expandDsTasksMention(text, [...plans.values()], scope);
      const reads: Array<{ status: string; reference: string | undefined }> = [
        ...input.args.taskRefs.map((reference) => ({ status: "all", reference })),
        ...input.args.groups.map((status) => ({ status, reference: undefined })),
      ];
      if (!reads.length) reads.push({ status: "not_cancelled", reference: undefined });
      for (const read of reads) {
        assertWebRequestContext(generation);
        const page = await fetchRequestedPlanTaskMetadata(api,plan.id,read.status,read.reference);
        assertWebRequestContext(generation);
        const revision = revisions.get(plan.id);
        if (revision && revision !== page.revision) throw new Error("Tasks changed while preparing the task list. Send it again to read the current list.");
        revisions.set(plan.id,page.revision);
        const previous = plans.get(plan.id)!;
        const tasks = new Map(previous.tasks.map((task) => [task.id,task]));
        for (const task of page.tasks) tasks.set(task.id,task);
        plans.set(plan.id,{...page,tasks:[...tasks.values()]});
      }
    }
    return expandDsTasksMention(text,[...plans.values()],scope);
  }, [muxStore, tabId, api?.host, api?.port, api?.token]);
  return { expand, plansSink: <PlansSink sink={plansRef} /> };
}
