import type { PlanView as Plan } from "../../components/Canvas/planProjection";
import { decodeTasksBlockKey, type TasksBlockGroup, type TasksBlockHost, type TasksBlockSpec, type TasksBlockState } from "@codeterm/chat-ui";
import { TASK_STATUS_COLORS } from "../../components/Canvas/settings";
import type { PlanTarget } from "../../components/Canvas/teamViewNavigation";
import type { TaskStatus } from "../../components/Canvas/types";
import { dsTaskProgressLabel, dsTaskRowsForSpec, groupDsTaskRows } from "../../components/Terminal/dsTasks/dsTasksModel";
import { formatModelBadge } from "../formatModelBadge";
import type { PlanProjection } from "../../components/Canvas/planProjection";
import type { ScopedPlanBlockRead } from "./scopedPlanBlockRead";
import type { ValueFeed } from "./valueFeed";

export interface PlansSnapshot {
  plans: readonly Plan[];
  loaded: boolean;
}

/** Rows keep the block's task set; status, executor, model and progress come live from the plan. */
export function tasksBlockState(snapshot: PlansSnapshot, spec: TasksBlockSpec): TasksBlockState {
  const plan = snapshot.plans.find((candidate) => candidate.id === spec.planId);
  if (!plan) return snapshot.loaded ? { kind: "missing" } : { kind: "loading" };
  const rows = dsTaskRowsForSpec(spec, plan);
  const groups: TasksBlockGroup[] = groupDsTaskRows(rows).map((group) => ({
    key: group.key,
    label: group.label,
    collapsedByDefault: group.collapsedByDefault,
    rows: group.rows.map((row) => ({
      id: row.id,
      key: row.key,
      name: row.name,
      status: row.status,
      color: TASK_STATUS_COLORS[row.status as TaskStatus] ?? null,
      executor: row.executor,
      model: row.model ? formatModelBadge(row.model) : null,
      progress: row.detached ? null : dsTaskProgressLabel(row),
      detached: row.detached,
    })),
  }));
  return { kind: "ready", title: plan.title, total: rows.length, groups };
}

export function createTasksBlockHost(plans: ValueFeed<PlansSnapshot>, navigate: (target: PlanTarget) => void, scoped?: ScopedPlanBlockRead<PlanProjection>): TasksBlockHost {
  let cachedFor: PlansSnapshot | null = null;
  const states = new Map<string, TasksBlockState>();
  const scopedStates=new WeakMap<object,TasksBlockState>();
  const loading:TasksBlockState={kind:"loading"};
  const missing:TasksBlockState={kind:"missing"};
  const teamOf = (planId: string) => plans.get().plans.find((plan) => plan.id === planId)?.team_id ?? undefined;
  return {
    binding: {
      subscribe: scoped?.subscribe ?? plans.subscribe,
      retain: scoped?.retain,
      read(key) {
        if(scoped){
          const current=scoped.read(key);
          const cached=scopedStates.get(current);if(cached)return cached;
          const spec=decodeTasksBlockKey(key);
          const state=current.data && spec ? tasksBlockState({plans:[current.data],loaded:true},spec) : current.error ? missing : loading;
          scopedStates.set(current,state);return state;
        }
        const snapshot = plans.get();
        if (snapshot !== cachedFor) {
          cachedFor = snapshot;
          states.clear();
        }
        let state = states.get(key);
        if (!state) {
          const spec = decodeTasksBlockKey(key);
          state = spec ? tasksBlockState(snapshot, spec) : { kind: "missing" };
          states.set(key, state);
        }
        return state;
      },
      refresh: (key) => scoped?.refresh(key),
    },
    openPlan: (planId) => navigate({ planId, teamId: teamOf(planId) }),
    openTask: (planId, taskId) => navigate({ planId, taskId, teamId: teamOf(planId) }),
  };
}
